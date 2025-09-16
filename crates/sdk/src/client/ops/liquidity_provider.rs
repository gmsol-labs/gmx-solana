use std::{num::NonZeroU64, ops::Deref};

use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, make_bundle_builder::MakeBundleBuilder,
    transaction_builder::TransactionBuilder, IntoAtomicGroup,
};
use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::{
    builders::{
        liquidity_provider::{
            CreateLpTokenController, DisableLpTokenController, InitializeLp, LpTokenKind,
            StakeLpToken, StakeLpTokenHint,
        },
        StoreProgram,
    },
    client::pull_oracle::{FeedIds, PullOraclePriceConsumer},
    utils::token_map::FeedAddressMap,
};

/// Operations for liquidity-provider program.
pub trait LiquidityProviderOps<C> {
    /// Initialize LP staking program.
    fn initialize_lp(
        &self,
        min_stake_value: u128,
        initial_apy: u128,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Create LP token controller for a specific token mint.
    fn create_lp_token_controller(
        &self,
        lp_token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Disable LP token controller for a specific token mint.
    fn disable_lp_token_controller(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Stake LP token.
    fn stake_lp_token(
        &self,
        store: &Pubkey,
        lp_token_kind: LpTokenKind,
        lp_token_mint: &Pubkey,
        oracle: &Pubkey,
        amount: NonZeroU64,
    ) -> StakeLpTokenBuilder<'_, C>;
}

impl<C: Deref<Target = impl Signer> + Clone> LiquidityProviderOps<C> for crate::Client<C> {
    fn initialize_lp(
        &self,
        min_stake_value: u128,
        initial_apy: u128,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = InitializeLp::builder()
            .payer(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .min_stake_value(min_stake_value)
            .initial_apy(initial_apy)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn create_lp_token_controller(
        &self,
        lp_token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = CreateLpTokenController::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_mint(*lp_token_mint)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn disable_lp_token_controller(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = DisableLpTokenController::builder()
            .authority(self.payer())
            .store_program(self.store_program_for_builders(store))
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_mint(*lp_token_mint)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn stake_lp_token(
        &self,
        store: &Pubkey,
        lp_token_kind: LpTokenKind,
        lp_token_mint: &Pubkey,
        oracle: &Pubkey,
        amount: NonZeroU64,
    ) -> StakeLpTokenBuilder<'_, C> {
        StakeLpTokenBuilder {
            client: self,
            builder: StakeLpToken::builder()
                .payer(self.payer())
                .amount(amount)
                .oracle(*oracle)
                .lp_token_kind(lp_token_kind)
                .lp_token_mint(*lp_token_mint)
                .lp_program(self.lp_program_for_builders().clone())
                .store_program(
                    StoreProgram::builder()
                        .id(*self.store_program_id())
                        .store(*store)
                        .build(),
                )
                .build(),
            hint: None,
        }
    }
}

/// Builder for LP token staking instructions.
pub struct StakeLpTokenBuilder<'a, C> {
    client: &'a crate::Client<C>,
    builder: StakeLpToken,
    hint: Option<StakeLpTokenHint>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> StakeLpTokenBuilder<'a, C> {
    /// Set a specific position ID instead of using random generation.
    pub fn with_position_id(mut self, position_id: u64) -> Self {
        self.builder = self.builder.with_position_id(position_id);
        self
    }

    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<StakeLpTokenHint> {
        if let Some(hint) = self.hint.as_ref() {
            return Ok(hint.clone());
        }
        let hint = StakeLpTokenHint::from_rpc_client_with(&self.builder, self.client.rpc()).await?;
        self.hint = Some(hint.clone());
        Ok(hint)
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let ag = self.builder.clone().into_atomic_group(&hint)?;
        let txn = self.client.store_transaction().pre_atomic_group(ag, true);
        Ok(txn)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for StakeLpTokenBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: gmsol_solana_utils::bundle_builder::BundleOptions,
    ) -> gmsol_solana_utils::Result<gmsol_solana_utils::bundle_builder::BundleBuilder<'a, C>> {
        let mut tx = self.client.bundle_with_options(options);

        tx.try_push(
            self.build_txn()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
        )?;

        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for StakeLpTokenBuilder<'_, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(
            self.builder.store_program.store.0,
            hint.to_tokens_with_feeds()?,
        ))
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.builder.insert_feed_parser(provider, map)?;
        Ok(())
    }
}
