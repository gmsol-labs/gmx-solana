use std::{future::Future, ops::Deref};

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use gmsol_treasury::states::treasury::TokenFlag;

use crate::utils::RpcBuilder;

/// Treasury instructions.
pub trait TreasuryOps<C> {
    /// Initialize [`Config`](crate::types::treasury::Config) account.
    fn initialize_config(&self, store: &Pubkey) -> RpcBuilder<C>;

    /// Set treasury.
    fn set_treasury(&self, store: &Pubkey, treasury_config: &Pubkey) -> RpcBuilder<C>;

    /// Set GT factor.
    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<RpcBuilder<C>>;

    /// Initialize [`TreasuryConfig`](crate::types::treasury::TreasuryConfig).
    fn initialize_treasury(&self, store: &Pubkey, index: u8) -> RpcBuilder<C>;

    /// Insert token to treasury.
    fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Remove token from treasury.
    fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Toggle token flag.
    fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Deposit into a treasury vault.
    fn deposit_into_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Withdraw from a treasury vault.
    #[allow(clippy::too_many_arguments)]
    fn withdraw_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Confirm GT buyback.
    fn confirm_gt_buyback(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: Option<&Pubkey>,
        token_map_hint: Option<&Pubkey>,
        oracle: &Pubkey,
        with_chainlink_program: bool,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Transfer receiver.
    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C>;

    /// Claim fees to receiver vault.
    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
    ) -> RpcBuilder<C>;

    /// Prepare GT bank.
    fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Sync GT bank.
    fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Complete GT exchange.
    fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;
}

impl<S, C> TreasuryOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn initialize_config(&self, store: &Pubkey) -> RpcBuilder<C> {
        todo!()
    }

    fn set_treasury(&self, store: &Pubkey, treasury_config: &Pubkey) -> RpcBuilder<C> {
        todo!()
    }

    fn set_gt_factor(&self, store: &Pubkey, factor: u128) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    fn initialize_treasury(&self, store: &Pubkey, index: u8) -> RpcBuilder<C> {
        todo!()
    }

    async fn insert_token_to_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn remove_token_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn toggle_token_flag(
        &self,
        store: &Pubkey,
        treasury_config: Option<&Pubkey>,
        token_mint: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn deposit_into_treasury_valut(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn withdraw_from_treasury(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        token_mint: &Pubkey,
        token_program_id: Option<&Pubkey>,
        amount: u64,
        decimals: u8,
        target: Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn confirm_gt_buyback(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: Option<&Pubkey>,
        token_map_hint: Option<&Pubkey>,
        oracle: &Pubkey,
        with_chainlink_program: bool,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C> {
        todo!()
    }

    fn claim_fees_to_receiver_vault(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        token_mint: &Pubkey,
    ) -> RpcBuilder<C> {
        todo!()
    }

    async fn prepare_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn sync_gt_bank(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
        token_mint: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }

    async fn complete_gt_exchange(
        &self,
        store: &Pubkey,
        treasury_config_hint: Option<&Pubkey>,
        gt_exchange_vault: &Pubkey,
    ) -> crate::Result<RpcBuilder<C>> {
        todo!()
    }
}
