use std::{num::NonZeroU64, ops::Deref};

use gmsol_solana_utils::{
    client_traits::FromRpcClientWith,
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
    IntoAtomicGroup,
};
use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::{
    builders::{
        liquidity_provider::{
            AcceptAuthority, ClaimGtReward, CreateLpTokenController, DisableLpTokenController,
            InitializeLp, LpTokenKind, SetClaimEnabled, SetPricingStaleness, StakeLpToken,
            StakeLpTokenHint, TransferAuthority, UnstakeLpToken, UpdateApyGradientRange,
            UpdateApyGradientSparse, UpdateMinStakeValue,
        },
        StoreProgram,
    },
    client::pull_oracle::{FeedIds, PullOraclePriceConsumer},
    utils::token_map::FeedAddressMap,
};

// ============================================================================
// Constants
// ============================================================================

/// Compute budget for LP token staking operations
const STAKE_LP_TOKEN_COMPUTE_BUDGET: u32 = 800_000;

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
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Disable LP token controller for a specific token mint.
    fn disable_lp_token_controller(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Unstake LP token.
    fn unstake_lp_token(
        &self,
        store: &Pubkey,
        lp_token_kind: LpTokenKind,
        lp_token_mint: &Pubkey,
        position_id: u64,
        unstake_amount: u64,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Stake LP token.
    fn stake_lp_token(
        &self,
        store: &Pubkey,
        lp_token_kind: LpTokenKind,
        lp_token_mint: &Pubkey,
        oracle: &Pubkey,
        amount: NonZeroU64,
        controller_index: u64,
    ) -> StakeLpTokenBuilder<'_, C>;

    /// Calculate GT reward for a position.
    fn calculate_gt_reward(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        owner: &Pubkey,
        position_id: u64,
        controller_index: u64,
    ) -> impl std::future::Future<Output = crate::Result<u128>>;

    /// Claim GT rewards for a position.
    fn claim_gt_reward(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        position_id: u64,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Transfer LP program authority to a new authority.
    fn transfer_lp_authority(
        &self,
        new_authority: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Accept LP program authority transfer.
    fn accept_lp_authority(&self) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Set whether claiming GT at any time is allowed.
    fn set_claim_enabled(&self, enabled: bool) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Set pricing staleness configuration.
    fn set_pricing_staleness(
        &self,
        staleness_seconds: u32,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Update APY gradient with sparse entries.
    fn update_apy_gradient_sparse(
        &self,
        bucket_indices: Vec<u8>,
        apy_values: Vec<u128>,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Update APY gradient for a contiguous range.
    fn update_apy_gradient_range(
        &self,
        start_bucket: u8,
        end_bucket: u8,
        apy_values: Vec<u128>,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Update minimum stake value.
    fn update_min_stake_value(
        &self,
        new_min_stake_value: u128,
    ) -> crate::Result<TransactionBuilder<'_, C>>;

    /// Query all LP staking positions for a specific owner.
    fn get_lp_positions(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
    ) -> impl std::future::Future<
        Output = crate::Result<Vec<crate::serde::serde_lp_position::SerdeLpStakingPosition>>,
    >;

    /// Query a specific LP staking position.
    fn get_lp_position(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
        position_id: u64,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> impl std::future::Future<
        Output = crate::Result<Option<crate::serde::serde_lp_position::SerdeLpStakingPosition>>,
    >;

    /// Query all LP staking positions for the current wallet.
    fn get_my_lp_positions(
        &self,
        store: &Pubkey,
    ) -> impl std::future::Future<
        Output = crate::Result<Vec<crate::serde::serde_lp_position::SerdeLpStakingPosition>>,
    >;
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
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = CreateLpTokenController::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_mint(*lp_token_mint)
            .controller_index(controller_index)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn disable_lp_token_controller(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = DisableLpTokenController::builder()
            .authority(self.payer())
            .store_program(self.store_program_for_builders(store))
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_mint(*lp_token_mint)
            .controller_index(controller_index)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn unstake_lp_token(
        &self,
        store: &Pubkey,
        lp_token_kind: LpTokenKind,
        lp_token_mint: &Pubkey,
        position_id: u64,
        unstake_amount: u64,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = UnstakeLpToken::builder()
            .payer(self.payer())
            .store_program(self.store_program_for_builders(store))
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_kind(lp_token_kind)
            .lp_token_mint(*lp_token_mint)
            .position_id(position_id)
            .unstake_amount(unstake_amount)
            .controller_index(controller_index)
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
        controller_index: u64,
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
                .controller_index(controller_index)
                .build(),
            hint: None,
        }
    }

    async fn calculate_gt_reward(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        owner: &Pubkey,
        position_id: u64,
        controller_index: u64,
    ) -> crate::Result<u128> {
        let lp_program = self.lp_program_for_builders();
        lp_program
            .calculate_gt_reward(
                self.rpc(),
                store,
                lp_token_mint,
                owner,
                position_id,
                controller_index,
            )
            .await
    }

    fn claim_gt_reward(
        &self,
        store: &Pubkey,
        lp_token_mint: &Pubkey,
        position_id: u64,
        controller_index: u64,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = ClaimGtReward::builder()
            .owner(self.payer())
            .store_program(self.store_program_for_builders(store))
            .lp_program(self.lp_program_for_builders().clone())
            .lp_token_mint(*lp_token_mint)
            .position_id(position_id)
            .controller_index(controller_index)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn transfer_lp_authority(
        &self,
        new_authority: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = TransferAuthority::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .new_authority(*new_authority)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn accept_lp_authority(&self) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = AcceptAuthority::builder()
            .pending_authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn set_claim_enabled(&self, enabled: bool) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = SetClaimEnabled::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .enabled(enabled)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn set_pricing_staleness(
        &self,
        staleness_seconds: u32,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = SetPricingStaleness::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .staleness_seconds(staleness_seconds)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn update_apy_gradient_sparse(
        &self,
        bucket_indices: Vec<u8>,
        apy_values: Vec<u128>,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = UpdateApyGradientSparse::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .bucket_indices(bucket_indices)
            .apy_values(apy_values)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn update_apy_gradient_range(
        &self,
        start_bucket: u8,
        end_bucket: u8,
        apy_values: Vec<u128>,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = UpdateApyGradientRange::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .start_bucket(start_bucket)
            .end_bucket(end_bucket)
            .apy_values(apy_values)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn update_min_stake_value(
        &self,
        new_min_stake_value: u128,
    ) -> crate::Result<TransactionBuilder<'_, C>> {
        let builder = UpdateMinStakeValue::builder()
            .authority(self.payer())
            .lp_program(self.lp_program_for_builders().clone())
            .new_min_stake_value(new_min_stake_value)
            .build();

        let ag = builder.into_atomic_group(&())?;
        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    async fn get_lp_positions(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
    ) -> crate::Result<Vec<crate::serde::serde_lp_position::SerdeLpStakingPosition>> {
        let lp_program = self.lp_program_for_builders();
        lp_program
            .query_lp_positions(self.rpc(), store, owner)
            .await
    }

    async fn get_lp_position(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
        position_id: u64,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> crate::Result<Option<crate::serde::serde_lp_position::SerdeLpStakingPosition>> {
        let lp_program = self.lp_program_for_builders();
        lp_program
            .query_lp_position(
                self.rpc(),
                store,
                owner,
                position_id,
                lp_token_mint,
                controller_index,
            )
            .await
    }

    async fn get_my_lp_positions(
        &self,
        store: &Pubkey,
    ) -> crate::Result<Vec<crate::serde::serde_lp_position::SerdeLpStakingPosition>> {
        self.get_lp_positions(store, &self.payer()).await
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
        let mut txn = self.client.store_transaction().pre_atomic_group(ag, true);
        txn.compute_budget_mut()
            .set_limit(STAKE_LP_TOKEN_COMPUTE_BUDGET);
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

impl<C> SetExecutionFee for StakeLpTokenBuilder<'_, C> {
    fn is_execution_fee_estimation_required(&self) -> bool {
        false
    }

    fn set_execution_fee(&mut self, _lamports: u64) -> &mut Self {
        // LP staking doesn't require execution fees, so this is a no-op
        self
    }
}
