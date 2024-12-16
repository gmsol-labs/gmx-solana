use std::{future::Future, ops::Deref};

use crate::utils::RpcBuilder;
use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_store::{
    accounts, instruction,
    states::gt::{get_time_window_index, GtExchange},
};

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
    ) -> RpcBuilder<C>;

    /// Configurate GT order fee dicounts.
    fn gt_set_order_fee_discount_factors(
        &self,
        store: &Pubkey,
        factors: Vec<u128>,
    ) -> RpcBuilder<C>;

    /// Configurate GT referral rewards
    fn gt_set_referral_reward_factors(&self, store: &Pubkey, factors: Vec<u128>) -> RpcBuilder<C>;

    /// Configurate the time window size for GT exchange.
    fn gt_set_exchange_time_window(&self, store: &Pubkey, window: u32) -> RpcBuilder<C>;

    /// Configurate the receiver of esGT vault.
    fn gt_set_es_receiver(&self, store: &Pubkey, receiver: &Pubkey) -> RpcBuilder<C>;

    /// Configurate the recevier factor of esGT.
    fn gt_set_es_receiver_factor(&self, store: &Pubkey, factor: u128) -> RpcBuilder<C>;

    /// Initialize GT exchange vault with the given time window index.
    fn prepare_gt_exchange_vault_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
    ) -> RpcBuilder<C, Pubkey>;

    /// Prepare GT exchange vault with the given time window.
    fn prepare_gt_exchange_vault_with_time_window(
        &self,
        store: &Pubkey,
        time_window: u32,
    ) -> crate::Result<RpcBuilder<C, Pubkey>> {
        Ok(self.prepare_gt_exchange_vault_with_time_window_index(
            store,
            current_time_window_index(time_window)?,
            time_window,
        ))
    }

    /// Confirm the given GT exchange vault.
    fn confirm_gt_exchange_vault(&self, store: &Pubkey, vault: &Pubkey) -> RpcBuilder<C>;

    /// Request GT exchange with the given time window index.
    fn request_gt_exchange_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        amount: u64,
    ) -> RpcBuilder<C>;

    /// Request GT exchange with the given time window.
    fn request_gt_exchange_with_time_window(
        &self,
        store: &Pubkey,
        time_window: u32,
        amount: u64,
    ) -> crate::Result<RpcBuilder<C>> {
        Ok(self.request_gt_exchange_with_time_window_index(
            store,
            current_time_window_index(time_window)?,
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
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Request GT vesting.
    fn request_gt_vesting(&self, store: &Pubkey, amount: u64) -> RpcBuilder<C>;

    /// Update vesting.
    fn update_gt_vesting(&self, store: &Pubkey) -> RpcBuilder<C>;

    /// Claim esGT.
    fn claim_es_gt(&self, store: &Pubkey) -> RpcBuilder<C>;

    /// Claim esGT vesting from vault.
    fn claim_es_vesting_from_vault(&self, store: &Pubkey, amount: u64) -> RpcBuilder<C>;
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
    ) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::InitializeGt {
                authority: self.payer(),
                store: *store,
                system_program: system_program::ID,
            })
            .args(instruction::InitializeGt {
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
    ) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfigurateGt {
                authority: self.payer(),
                store: *store,
            })
            .args(instruction::GtSetOrderFeeDiscountFactors { factors })
    }

    fn gt_set_referral_reward_factors(&self, store: &Pubkey, factors: Vec<u128>) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfigurateGt {
                authority: self.payer(),
                store: *store,
            })
            .args(instruction::GtSetReferralRewardFactors { factors })
    }

    fn gt_set_exchange_time_window(&self, store: &Pubkey, window: u32) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfigurateGt {
                authority: self.payer(),
                store: *store,
            })
            .args(instruction::GtSetExchangeTimeWindow { window })
    }

    fn gt_set_es_receiver(&self, store: &Pubkey, receiver: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfigurateGt {
                authority: self.payer(),
                store: *store,
            })
            .args(instruction::GtSetReceiver {
                receiver: *receiver,
            })
    }

    fn gt_set_es_receiver_factor(&self, store: &Pubkey, factor: u128) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfigurateGt {
                authority: self.payer(),
                store: *store,
            })
            .args(instruction::GtSetEsReceiverFactor { factor })
    }

    fn prepare_gt_exchange_vault_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
    ) -> RpcBuilder<C, Pubkey> {
        let vault = self.find_gt_exchange_vault_address(store, time_window_index);
        self.store_rpc()
            .accounts(accounts::PrepareGtExchangeVault {
                payer: self.payer(),
                store: *store,
                vault,
                system_program: system_program::ID,
            })
            .args(instruction::PrepareGtExchangeVault {
                time_window_index,
                time_window,
            })
            .with_output(vault)
    }

    fn confirm_gt_exchange_vault(&self, store: &Pubkey, vault: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .accounts(accounts::ConfirmGtExchangeVault {
                authority: self.payer(),
                store: *store,
                vault: *vault,
            })
            .args(instruction::ConfirmGtExchangeVault {})
    }

    fn request_gt_exchange_with_time_window_index(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        amount: u64,
    ) -> RpcBuilder<C> {
        let owner = self.payer();
        let vault = self.find_gt_exchange_vault_address(store, time_window_index);
        self.store_rpc()
            .accounts(accounts::RequestGtExchange {
                owner,
                store: *store,
                user: self.find_user_address(store, &owner),
                vault,
                exchange: self.find_gt_exchange_address(&vault, &owner),
                system_program: system_program::ID,
            })
            .args(instruction::RequestGtExchange { amount })
    }

    async fn close_gt_exchange(
        &self,
        store: &Pubkey,
        exchange: &Pubkey,
        hint_owner: Option<&Pubkey>,
        hint_vault: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        let (owner, vault) = match (hint_owner, hint_vault) {
            (Some(owner), Some(vault)) => (*owner, *vault),
            _ => {
                let exchange = self
                    .account::<GtExchange>(exchange)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                (*exchange.owner(), *exchange.vault())
            }
        };

        Ok(self
            .store_rpc()
            .accounts(accounts::CloseGtExchange {
                authority: self.payer(),
                store: *store,
                owner,
                vault,
                exchange: *exchange,
            })
            .args(instruction::CloseGtExchange {}))
    }

    fn request_gt_vesting(&self, store: &Pubkey, amount: u64) -> RpcBuilder<C> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let vesting = self.find_gt_vesting_address(store, &owner);
        self.store_rpc()
            .accounts(accounts::RequestGtVesting {
                owner,
                store: *store,
                user,
                vesting,
                system_program: system_program::ID,
            })
            .args(instruction::RequestGtVesting { amount })
    }

    fn update_gt_vesting(&self, store: &Pubkey) -> RpcBuilder<C> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let vesting = self.find_gt_vesting_address(store, &owner);
        self.store_rpc()
            .accounts(accounts::UpdateGtVesting {
                owner,
                store: *store,
                user,
                vesting,
            })
            .args(instruction::UpdateGtVesting {})
    }

    fn claim_es_gt(&self, store: &Pubkey) -> RpcBuilder<C> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        self.store_rpc()
            .accounts(accounts::ClaimEsGt {
                owner,
                store: *store,
                user,
            })
            .args(instruction::ClaimEsGt {})
    }

    fn claim_es_vesting_from_vault(&self, store: &Pubkey, amount: u64) -> RpcBuilder<C> {
        let prepare = self.request_gt_vesting(store, 0);
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let vesting = self.find_gt_vesting_address(store, &owner);
        let rpc = self
            .store_rpc()
            .accounts(accounts::ClaimEsGtVaultViaVesting {
                owner,
                store: *store,
                user,
                vesting,
            })
            .args(instruction::ClaimEsGtVaultViaVesting { amount });
        prepare.merge(rpc)
    }
}

/// Get current time window index.
pub fn current_time_window_index(time_window: u32) -> crate::Result<i64> {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(crate::Error::invalid_argument)?;

    let ts = now.as_secs() as i64;
    Ok(get_time_window_index(ts, time_window as i64))
}
