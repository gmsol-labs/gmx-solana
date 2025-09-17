use gmsol_sdk::{
    builders::liquidity_provider::LpTokenKind,
    ops::{token_account::TokenAccountOps, user::UserOps},
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::make_bundle_builder::MakeBundleBuilder,
    utils::Value,
};
use std::num::NonZeroU64;

/// Liquidity Provider management commands.
#[derive(Debug, clap::Args)]
pub struct Lp {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Initialize LP staking program.
    InitLp {
        /// Minimum stake value.
        #[arg(long)]
        min_stake_value: Value,
        /// Initial APY.
        #[arg(long)]
        initial_apy: Value,
    },
    /// Create LP token controller for a specific token mint.
    CreateController {
        /// LP token mint address.
        lp_token_mint: Pubkey,
    },
    /// Disable LP token controller for a specific token mint.
    DisableController {
        /// LP token mint address.
        lp_token_mint: Pubkey,
    },
    /// Stake LP tokens (GM or GLV).
    Stake {
        /// LP token kind (GM or GLV).
        #[arg(long, value_enum)]
        kind: LpTokenKind,
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Amount to stake (in raw token units).
        #[arg(long)]
        amount: u64,
        /// Optional position ID (if not provided, will generate randomly).
        #[arg(long)]
        position_id: Option<u64>,
    },
    /// Unstake LP tokens (GM or GLV).
    Unstake {
        /// LP token kind (GM or GLV).
        #[arg(long, value_enum)]
        kind: LpTokenKind,
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Position ID to unstake from.
        #[arg(long)]
        position_id: u64,
        /// Amount to unstake (in raw token units).
        #[arg(long)]
        amount: u64,
    },
    /// Calculate GT reward for a position.
    CalculateReward {
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Position ID to calculate reward for.
        #[arg(long)]
        position_id: u64,
        /// Owner of the position (optional, defaults to current payer).
        #[arg(long)]
        owner: Option<Pubkey>,
    },
    /// Transfer LP program authority to a new authority.
    TransferAuthority {
        /// New authority address.
        new_authority: Pubkey,
    },
    /// Accept LP program authority transfer.
    AcceptAuthority,
    /// Set whether claiming GT at any time is allowed.
    SetClaimEnabled {
        /// Whether to enable claiming.
        #[arg(long)]
        enable: bool,
    },
    /// Set pricing staleness configuration.
    SetPricingStaleness {
        /// Staleness threshold in seconds.
        staleness_seconds: u32,
    },
    /// Update APY gradient with sparse entries.
    UpdateApyGradientSparse {
        /// Bucket indices to update.
        #[arg(long, value_delimiter = ',')]
        bucket_indices: Vec<u8>,
        /// APY values (percentages, will be converted to 1e20-scaled).
        #[arg(long, value_delimiter = ',')]
        apy_values: Vec<Value>,
    },
    /// Update APY gradient for a contiguous range.
    UpdateApyGradientRange {
        /// Start bucket index.
        start_bucket: u8,
        /// End bucket index.  
        end_bucket: u8,
        /// APY values (percentages, will be converted to 1e20-scaled).
        #[arg(long, value_delimiter = ',')]
        apy_values: Vec<Value>,
    },
}

impl super::Command for Lp {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::InitLp {
                min_stake_value,
                initial_apy,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                client
                    .initialize_lp(min_stake_value.to_u128()?, initial_apy.to_u128()?)?
                    .into_bundle_with_options(options)?
            }
            Command::CreateController { lp_token_mint } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                client
                    .create_lp_token_controller(lp_token_mint)?
                    .into_bundle_with_options(options)?
            }
            Command::DisableController { lp_token_mint } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                client
                    .disable_lp_token_controller(store, lp_token_mint)?
                    .into_bundle_with_options(options)?
            }
            Command::Stake {
                kind,
                lp_token_mint,
                amount,
                position_id,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Get oracle from global config
                let oracle = ctx.config().oracle()?;

                // Convert amount to NonZeroU64
                let stake_amount = NonZeroU64::new(*amount)
                    .ok_or_else(|| eyre::eyre!("Stake amount must be greater than zero"))?;

                // Create stake builder
                let mut stake_builder =
                    client.stake_lp_token(store, *kind, lp_token_mint, oracle, stake_amount);

                // Set position ID if provided
                if let Some(pos_id) = position_id {
                    stake_builder = stake_builder.with_position_id(*pos_id);
                }

                // Build the bundle
                stake_builder.build_with_options(options).await?
            }
            Command::Unstake {
                kind,
                lp_token_mint,
                position_id,
                amount,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Prepare GT user account (idempotent operation)
                let prepare_user = client.prepare_user(store)?;

                // Determine correct token program ID (GM uses token::ID, GLV uses token_2022::ID)
                let token_program_id = match kind {
                    LpTokenKind::Gm => anchor_spl::token::ID,
                    LpTokenKind::Glv => anchor_spl::token_2022::ID,
                };

                // Prepare destination ATA (idempotent operation)
                let prepare_ata = client.prepare_associated_token_account(
                    lp_token_mint,
                    &token_program_id,
                    None, // Use current payer as owner
                );

                // Create unstake transaction
                let unstake_tx =
                    client.unstake_lp_token(store, *kind, lp_token_mint, *position_id, *amount)?;

                // Merge all transactions and build bundle
                prepare_user
                    .merge(prepare_ata)
                    .merge(unstake_tx)
                    .into_bundle_with_options(options)?
            }
            Command::CalculateReward {
                lp_token_mint,
                position_id,
                owner,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Use provided owner or default to current payer
                let position_owner = owner.unwrap_or_else(|| client.payer());

                // Create calculate GT reward transaction using SDK
                client
                    .calculate_gt_reward(store, lp_token_mint, &position_owner, *position_id)?
                    .into_bundle_with_options(options)?
            }
            Command::TransferAuthority { new_authority } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Transfer LP program authority
                client
                    .transfer_lp_authority(new_authority)?
                    .into_bundle_with_options(options)?
            }
            Command::AcceptAuthority => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Accept LP program authority transfer
                client
                    .accept_lp_authority()?
                    .into_bundle_with_options(options)?
            }
            Command::SetClaimEnabled { enable } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Set claim enabled status
                client
                    .set_claim_enabled(*enable)?
                    .into_bundle_with_options(options)?
            }
            Command::SetPricingStaleness { staleness_seconds } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Set pricing staleness configuration
                client
                    .set_pricing_staleness(*staleness_seconds)?
                    .into_bundle_with_options(options)?
            }
            Command::UpdateApyGradientSparse {
                bucket_indices,
                apy_values,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Validate input lengths match
                if bucket_indices.len() != apy_values.len() {
                    return Err(eyre::eyre!(
                        "bucket_indices and apy_values must have the same length"
                    ));
                }

                // Convert APY percentages to 1e20-scaled values
                let apy_values_scaled = apy_values
                    .iter()
                    .map(|v| v.to_u128().map_err(eyre::Error::from))
                    .collect::<eyre::Result<Vec<_>>>()?;

                // Update APY gradient with sparse entries
                client
                    .update_apy_gradient_sparse(bucket_indices.clone(), apy_values_scaled)?
                    .into_bundle_with_options(options)?
            }
            Command::UpdateApyGradientRange {
                start_bucket,
                end_bucket,
                apy_values,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Validate range
                if start_bucket > end_bucket {
                    return Err(eyre::eyre!("start_bucket must be <= end_bucket"));
                }

                let expected_length = (end_bucket - start_bucket + 1) as usize;
                if apy_values.len() != expected_length {
                    return Err(eyre::eyre!(
                        "apy_values length ({}) must match range size ({})",
                        apy_values.len(),
                        expected_length
                    ));
                }

                // Convert APY percentages to 1e20-scaled values
                let apy_values_scaled = apy_values
                    .iter()
                    .map(|v| v.to_u128().map_err(eyre::Error::from))
                    .collect::<eyre::Result<Vec<_>>>()?;

                // Update APY gradient for range
                client
                    .update_apy_gradient_range(*start_bucket, *end_bucket, apy_values_scaled)?
                    .into_bundle_with_options(options)?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
