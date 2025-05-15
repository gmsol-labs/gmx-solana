use std::{future::Future, ops::Deref};

use gmsol_programs::gmsol_store::{
    accounts::GtExchange,
    client::{accounts, args},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::gt::get_time_window_index;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

use crate::utils::zero_copy::ZeroCopy;

/// GT Operations.
pub trait GtOps<C> {
    /// Initialize GT Mint.
    fn initialize_gt(
        &self,
        store: &Pubkey,
        decimals: u8,
        initial_minting_cost: u128,
        grow_factor: u128,
        grow_step: u64,
        ranks: Vec<u64>,
    ) -> TransactionBuilder<C>;

    /// Configurate GT order fee dicounts.
    fn gt_set_order_fee_discount_factors(
        &self,
        store: &Pubkey,
        factors: Vec<u128>,
    ) -> TransactionBuilder<C>;

    /// Configurate GT referral rewards
    fn gt_set_referral_reward_factors(
        &self,
        store: &Pubkey,
        factors: Vec<u128>,
    ) -> TransactionBuilder<C>;

    /// Configurate the time window size for GT exchange.
    fn gt_set_exchange_time_window(&self, store: &Pubkey, window: u32) -> TransactionBuilder<C>;

    /// Initialize GT exchange vault with the given time window index.
    fn prepare_gt_exchange_vault_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Prepare GT exchange vault with the given time window.
    fn prepare_gt_exchange_vault_with_time_window(
        &self,
        store: &Pubkey,
        time_window: u32,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        Ok(self.prepare_gt_exchange_vault_with_time_window_index(
            store,
            current_time_window_index(time_window)?,
            time_window,
        ))
    }

    /// Confirm the given GT exchange vault.
    fn confirm_gt_exchange_vault(&self, store: &Pubkey, vault: &Pubkey) -> TransactionBuilder<C>;

    /// Request GT exchange with the given time window index.
    fn request_gt_exchange_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
        amount: u64,
    ) -> TransactionBuilder<C>;

    /// Request GT exchange with the given time window.
    fn request_gt_exchange_with_time_window(
        &self,
        store: &Pubkey,
        time_window: u32,
        amount: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        Ok(self.request_gt_exchange_with_time_window_index(
            store,
            current_time_window_index(time_window)?,
            time_window,
            amount,
        ))
    }

    /// Close a confirmed GT exchange.
    fn close_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        hint_owner: Option<&Pubkey>,
        hint_vault: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;
}

impl<C: Deref<Target = impl Signer> + Clone> GtOps<C> for crate::Client<C> {
    fn initialize_gt(
        &self,
        store: &Pubkey,
        decimals: u8,
        initial_minting_cost: u128,
        grow_factor: u128,
        grow_step: u64,
        ranks: Vec<u64>,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::InitializeGt {
                authority: self.payer(),
                store: *store,
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializeGt {
                decimals,
                initial_minting_cost,
                grow_factor,
                grow_step,
                ranks,
            })
    }

    fn gt_set_order_fee_discount_factors(
        &self,
        store: &Pubkey,
        factors: Vec<u128>,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::GtSetOrderFeeDiscountFactors {
                authority: self.payer(),
                store: *store,
            })
            .anchor_args(args::GtSetOrderFeeDiscountFactors { factors })
    }

    fn gt_set_referral_reward_factors(
        &self,
        store: &Pubkey,
        factors: Vec<u128>,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::GtSetReferralRewardFactors {
                authority: self.payer(),
                store: *store,
            })
            .anchor_args(args::GtSetReferralRewardFactors { factors })
    }

    fn gt_set_exchange_time_window(&self, store: &Pubkey, window: u32) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::GtSetExchangeTimeWindow {
                authority: self.payer(),
                store: *store,
            })
            .anchor_args(args::GtSetExchangeTimeWindow { window })
    }

    fn prepare_gt_exchange_vault_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
    ) -> TransactionBuilder<C, Pubkey> {
        let vault = self.find_gt_exchange_vault_address(store, time_window_index, time_window);
        self.store_transaction()
            .anchor_accounts(accounts::PrepareGtExchangeVault {
                payer: self.payer(),
                store: *store,
                vault,
                system_program: system_program::ID,
            })
            .anchor_args(args::PrepareGtExchangeVault { time_window_index })
            .output(vault)
    }

    fn confirm_gt_exchange_vault(&self, store: &Pubkey, vault: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::ConfirmGtExchangeVault {
                authority: self.payer(),
                store: *store,
                vault: *vault,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::ConfirmGtExchangeVault {})
    }

    fn request_gt_exchange_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
        amount: u64,
    ) -> TransactionBuilder<C> {
        let owner = self.payer();
        let vault = self.find_gt_exchange_vault_address(store, time_window_index, time_window);
        self.store_transaction()
            .anchor_accounts(accounts::RequestGtExchange {
                owner,
                store: *store,
                user: self.find_user_address(store, &owner),
                vault,
                exchange: self.find_gt_exchange_address(&vault, &owner),
                system_program: system_program::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::RequestGtExchange { amount })
    }

    async fn close_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        hint_owner: Option<&Pubkey>,
        hint_vault: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let (owner, vault) = match (hint_owner, hint_vault) {
            (Some(owner), Some(vault)) => (*owner, *vault),
            _ => {
                let exchange = self
                    .account::<ZeroCopy<GtExchange>>(exchange)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                (exchange.owner, exchange.vault)
            }
        };

        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::CloseGtExchange {
                authority: self.payer(),
                store: *store,
                owner,
                vault,
                exchange: *exchange,
            })
            .anchor_args(args::CloseGtExchange {}))
    }
}

/// Get current time window index.
pub fn current_time_window_index(time_window: u32) -> crate::Result<i64> {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(crate::Error::custom)?;

    let ts = now.as_secs() as i64;
    Ok(get_time_window_index(ts, time_window as i64))
}
