use gmsol_sdk::{
    builders::liquidity_provider::LpTokenKind,
    ops::{
        liquidity_provider::LiquidityProviderOps, token_account::TokenAccountOps, user::UserOps,
    },
    programs::anchor_lang::prelude::Pubkey,
    utils::{GmAmount, Value},
};

#[cfg(feature = "execute")]
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
    #[cfg(feature = "execute")]
    Stake {
        /// LP token kind (GM or GLV).
        #[arg(long, value_enum)]
        kind: LpTokenKind,
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Amount to stake (in GM/GLV token units, will be converted to raw units).
        #[arg(long)]
        amount: GmAmount,
        /// Optional position ID (if not provided, will generate randomly).
        #[arg(long)]
        position_id: Option<u64>,
        /// Executor arguments for oracle handling.
        #[command(flatten)]
        args: crate::commands::exchange::executor::ExecutorArgs,
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
        /// Amount to unstake (in GM/GLV token units, will be converted to raw units).
        #[arg(long)]
        amount: GmAmount,
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
    /// Claim GT rewards for a position.
    ClaimGt {
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Position ID to claim rewards for.
        #[arg(long)]
        position_id: u64,
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
    /// Update minimum stake value.
    UpdateMinStakeValue {
        /// New minimum stake value.
        new_min_stake_value: Value,
    },
    /// Query all LP staking positions for the current wallet.
    QueryMyPositions,
    /// Query all LP staking positions for a specified wallet.
    QueryPositions {
        /// Owner wallet address to query positions for.
        owner: Pubkey,
    },
    /// Query a specific LP staking position.
    QueryPosition {
        /// Owner of the position.
        owner: Pubkey,
        /// Position ID to query.
        #[arg(long)]
        position_id: u64,
        /// LP token mint address.
        lp_token_mint: Pubkey,
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
                // Use a placeholder for gt_mint since it's not actually used in the program logic
                let placeholder_gt_mint = Pubkey::default();
                client
                    .initialize_lp(min_stake_value.to_u128()?, initial_apy.to_u128()?)?
                    .into_bundle_with_options(options)?
            }
            Command::CreateController { lp_token_mint } => client
                .create_lp_token_controller(lp_token_mint)?
                .into_bundle_with_options(options)?,
            Command::DisableController { lp_token_mint } => client
                .disable_lp_token_controller(store, lp_token_mint)?
                .into_bundle_with_options(options)?,
            #[cfg(feature = "execute")]
            Command::Stake {
                kind,
                lp_token_mint,
                amount,
                position_id,
                args,
            } => {
                // Ensure we're not in instruction buffer mode since executor needs to send transactions
                ctx.require_not_ix_buffer_mode()?;

                // Get oracle from global config
                let oracle = ctx.config().oracle()?;

                // Convert GmAmount to u64 and then to NonZeroU64
                let amount_u64 = amount.to_u64()?;
                let stake_amount = NonZeroU64::new(amount_u64)
                    .ok_or_else(|| eyre::eyre!("Stake amount must be greater than zero"))?;

                // Create stake builder with position ID if provided
                let builder = match position_id {
                    Some(pos_id) => client
                        .stake_lp_token(store, *kind, lp_token_mint, oracle, stake_amount)
                        .with_position_id(*pos_id),
                    None => {
                        client.stake_lp_token(store, *kind, lp_token_mint, oracle, stake_amount)
                    }
                };

                // Use ExecutorArgs to build executor (same pattern as ConfirmGtBuyback in treasury)
                let executor = args.build(client).await?;
                executor.execute(builder, options).await?;
                return Ok(());
            }
            Command::Unstake {
                kind,
                lp_token_mint,
                position_id,
                amount,
            } => {
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

                // Convert GmAmount to u64
                let amount_u64 = amount.to_u64()?;

                // Create unstake transaction
                let unstake_tx = client.unstake_lp_token(
                    store,
                    *kind,
                    lp_token_mint,
                    *position_id,
                    amount_u64,
                )?;

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
                // Use provided owner or default to current payer
                let position_owner = owner.unwrap_or_else(|| client.payer());

                // Create calculate GT reward transaction using SDK
                client
                    .calculate_gt_reward(store, lp_token_mint, &position_owner, *position_id)?
                    .into_bundle_with_options(options)?
            }
            Command::ClaimGt {
                lp_token_mint,
                position_id,
            } => {
                // Prepare GT user account (idempotent operation) - same as Unstake
                let prepare_user = client.prepare_user(store)?;

                // Create claim GT reward transaction using SDK
                let claim_tx = client.claim_gt_reward(store, lp_token_mint, *position_id)?;

                // Merge prepare user and claim transactions
                prepare_user
                    .merge(claim_tx)
                    .into_bundle_with_options(options)?
            }
            Command::TransferAuthority { new_authority } => {
                // Transfer LP program authority
                client
                    .transfer_lp_authority(new_authority)?
                    .into_bundle_with_options(options)?
            }
            Command::AcceptAuthority => {
                // Accept LP program authority transfer
                client
                    .accept_lp_authority()?
                    .into_bundle_with_options(options)?
            }
            Command::SetClaimEnabled { enable } => {
                // Set claim enabled status
                client
                    .set_claim_enabled(*enable)?
                    .into_bundle_with_options(options)?
            }
            Command::SetPricingStaleness { staleness_seconds } => {
                // Set pricing staleness configuration
                client
                    .set_pricing_staleness(*staleness_seconds)?
                    .into_bundle_with_options(options)?
            }
            Command::UpdateApyGradientSparse {
                bucket_indices,
                apy_values,
            } => {
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
            Command::UpdateMinStakeValue {
                new_min_stake_value,
            } => {
                // Convert the value to 1e20-scaled u128
                let min_stake_value_scaled =
                    new_min_stake_value.to_u128().map_err(eyre::Error::from)?;

                // Update minimum stake value
                client
                    .update_min_stake_value(min_stake_value_scaled)?
                    .into_bundle_with_options(options)?
            }
            Command::QueryMyPositions => {
                // Query all positions for current wallet
                let positions = client.get_my_lp_positions(store).await?;
                let output = &ctx.config().output();
                self.display_positions_list(&positions, output)?;
                return Ok(());
            }
            Command::QueryPositions { owner } => {
                // Query all positions for specified owner
                let positions = client.get_lp_positions(store, owner).await?;
                let output = &ctx.config().output();
                self.display_positions_list(&positions, output)?;
                return Ok(());
            }
            Command::QueryPosition {
                owner,
                position_id,
                lp_token_mint,
            } => {
                // Query specific position
                let position = client
                    .get_lp_position(store, owner, *position_id, lp_token_mint)
                    .await?;

                let output = &ctx.config().output();
                match position {
                    Some(pos) => {
                        self.display_single_position(&pos, output)?;
                    }
                    None => {
                        println!(
                            "Position not found: owner={}, position_id={}, lp_token_mint={}",
                            owner, position_id, lp_token_mint
                        );
                    }
                }
                return Ok(());
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

impl Lp {
    /// Display a list of LP staking positions.
    /// Table format: LP token, amount, staked time, APY, claimable GT
    fn display_positions_list(
        &self,
        positions: &[gmsol_sdk::serde::serde_lp_position::SerdeLpStakingPosition],
        output: &crate::config::OutputFormat,
    ) -> eyre::Result<()> {
        use crate::config::DisplayOptions;

        if positions.is_empty() {
            println!("No LP staking positions found.");
            return Ok(());
        }

        // Create formatted positions with 2-decimal APY
        let formatted_positions: Vec<_> = positions
            .iter()
            .map(|pos| {
                let mut formatted = serde_json::to_value(pos).unwrap();
                if let Some(obj) = formatted.as_object_mut() {
                    // Format APY to 2 decimal places
                    if let Some(apy_value) = obj.get("current_apy") {
                        if let Some(apy_str) = apy_value.as_str() {
                            if let Ok(apy_num) = apy_str.parse::<f64>() {
                                obj.insert(
                                    "current_apy".to_string(),
                                    serde_json::Value::String(format!("{:.2}", apy_num)),
                                );
                            }
                        }
                    }
                    // Format GT to 4 decimal places
                    if let Some(gt_value) = obj.get("claimable_gt") {
                        if let Some(gt_str) = gt_value.as_str() {
                            if let Ok(gt_num) = gt_str.parse::<f64>() {
                                obj.insert(
                                    "claimable_gt".to_string(),
                                    serde_json::Value::String(format!("{:.4}", gt_num)),
                                );
                            }
                        }
                    }
                }
                formatted
            })
            .collect();

        let options = DisplayOptions::table_projection([
            ("position_id", "Position ID"),
            ("lp_token_symbol", "LP Token"),
            ("staked_amount", "Amount"),
            ("stake_start_time", "Staked Time"),
            ("current_apy", "APY"),
            ("claimable_gt", "Claimable GT"),
        ])
        .set_empty_message("No LP staking positions found.");

        println!("{}", output.display_many(formatted_positions, options)?);
        Ok(())
    }

    /// Display a single LP staking position with detailed information.
    /// Single format: LP token, amount, staked time, APY, accumulated GT, claimable GT
    fn display_single_position(
        &self,
        position: &gmsol_sdk::serde::serde_lp_position::SerdeLpStakingPosition,
        output: &crate::config::OutputFormat,
    ) -> eyre::Result<()> {
        use crate::config::DisplayOptions;

        // Format single position with 2-decimal APY
        let mut formatted = serde_json::to_value(position).unwrap();
        if let Some(obj) = formatted.as_object_mut() {
            // Format APY to 2 decimal places
            if let Some(apy_value) = obj.get("current_apy") {
                if let Some(apy_str) = apy_value.as_str() {
                    if let Ok(apy_num) = apy_str.parse::<f64>() {
                        obj.insert(
                            "current_apy".to_string(),
                            serde_json::Value::String(format!("{:.2}", apy_num)),
                        );
                    }
                }
            }
            // Format GT to 4 decimal places
            if let Some(gt_value) = obj.get("claimable_gt") {
                if let Some(gt_str) = gt_value.as_str() {
                    if let Ok(gt_num) = gt_str.parse::<f64>() {
                        obj.insert(
                            "claimable_gt".to_string(),
                            serde_json::Value::String(format!("{:.4}", gt_num)),
                        );
                    }
                }
            }
        }

        // Single position display: Position ID, LP token, amount, staked time, APY, claimable GT
        let options = DisplayOptions::table_projection([
            ("position_id", "Position ID"),
            ("lp_token_symbol", "LP Token"),
            ("staked_amount", "Amount"),
            ("stake_start_time", "Staked Time"),
            ("current_apy", "APY"),
            ("claimable_gt", "Claimable GT"),
        ]);

        // For single position, display as single item
        println!(
            "{}",
            output.display_many(std::iter::once(formatted), options)?
        );
        Ok(())
    }
}
