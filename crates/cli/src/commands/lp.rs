use gmsol_sdk::{
    builders::liquidity_provider::{
        GtRewardCalculationParams, LpPositionQueryParams, LpTokenKind, UnstakeLpTokenParams,
    },
    ops::{
        liquidity_provider::LiquidityProviderOps, token_account::TokenAccountOps, user::UserOps,
    },
    programs::{anchor_lang::prelude::Pubkey, gmsol_store::accounts::Store},
    utils::{zero_copy::ZeroCopy, GmAmount, Value},
};

#[cfg(feature = "execute")]
use std::num::NonZeroU64;

#[cfg(feature = "execute")]
use gmsol_sdk::builders::liquidity_provider::StakeLpTokenParams;

use crate::config::DisplayOptions;

// ============================================================================
// Constants
// ============================================================================

/// Number of decimal places for APY display formatting
const APY_DISPLAY_DECIMALS: usize = 2;

/// Maximum number of decimal places for GT amount display formatting
/// Actual GT decimals are dynamically retrieved from store, but limited to this max for readability
const GT_DISPLAY_DECIMALS: usize = 4;

// ============================================================================
// Commands
// ============================================================================

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
        /// Controller index.
        #[arg(long)]
        controller_index: u64,
    },
    /// Disable LP token controller for a specific token mint.
    DisableController {
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Controller index.
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
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
        /// Controller index.
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
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
        /// Controller index.
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
    },
    /// Calculate GT reward for a position.
    CalculateReward {
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Position ID to calculate reward for.
        #[arg(long)]
        position_id: u64,
        /// Owner of the position.
        #[arg(long)]
        owner: Pubkey,
        /// Controller index.
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
    },
    /// Claim GT rewards for a position.
    ClaimGt {
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Position ID to claim rewards for.
        #[arg(long)]
        position_id: u64,
        /// Controller index.
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
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
        #[arg(long, value_delimiter = ',', required = true)]
        apy_values: Vec<Value>,
    },
    /// Update APY gradient for a contiguous range.
    UpdateApyGradientRange {
        /// Start bucket index.
        start_bucket: u8,
        /// End bucket index.  
        end_bucket: u8,
        /// APY values (percentages, will be converted to 1e20-scaled).
        #[arg(long, value_delimiter = ',', required = true)]
        apy_values: Vec<Value>,
    },
    /// Update minimum stake value.
    UpdateMinStakeValue {
        /// New minimum stake value.
        new_min_stake_value: Value,
    },
    /// Query LP staking positions.
    QueryPositions {
        /// Owner wallet address to query positions for.
        /// If not provided, queries current wallet's positions.
        #[arg(long)]
        owner: Option<Pubkey>,
        /// Position ID to query (for specific position query).
        /// If provided, queries a specific position.
        #[arg(long)]
        position_id: Option<u64>,
        /// LP token mint address (required for specific position query).
        #[arg(long)]
        lp_token_mint: Option<Pubkey>,
        /// Controller index (for specific position query).
        #[arg(long)]
        controller_index: Option<u64>,
        /// Controller address (if provided, takes precedence over controller_index).
        #[arg(long)]
        controller_address: Option<Pubkey>,
    },
    /// Query LP token controllers for a specific token mint or all controllers.
    QueryControllers {
        /// LP token mint address (GM or GLV token). If not provided, returns all controllers.
        lp_token_mint: Option<Pubkey>,
    },
    /// Query LP Global State (authority, APY gradient, min stake value).
    QueryGlobalState,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Validates and resolves controller parameters.
///
/// # Arguments
/// * `controller_index` - Optional controller index
/// * `controller_address` - Optional controller address
///
/// # Returns
/// * `Ok((final_controller_index, final_controller_address))` - Resolved controller parameters
/// * `Err` - If both parameters are None
///
/// # Note
/// If controller_address is provided, it takes precedence over controller_index.
/// The final_controller_index will be set to 0 when using controller_address.
fn resolve_controller_params(
    controller_index: Option<u64>,
    controller_address: Option<Pubkey>,
) -> eyre::Result<(u64, Option<Pubkey>)> {
    // Validate that at least one controller parameter is provided
    if controller_index.is_none() && controller_address.is_none() {
        return Err(eyre::eyre!(
            "Must provide either --controller-index or --controller-address"
        ));
    }

    // Determine which controller to use (address takes precedence)
    let (final_controller_index, final_controller_address) = match controller_address {
        Some(addr) => (0, Some(addr)), // Use address, set index to 0 (will be ignored)
        None => (controller_index.unwrap(), None), // Use index
    };

    Ok((final_controller_index, final_controller_address))
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
            } => client
                .initialize_lp(min_stake_value.to_u128()?, initial_apy.to_u128()?)?
                .into_bundle_with_options(options)?,
            Command::CreateController {
                lp_token_mint,
                controller_index,
            } => client
                .create_lp_token_controller(lp_token_mint, *controller_index)?
                .into_bundle_with_options(options)?,
            Command::DisableController {
                lp_token_mint,
                controller_index,
                controller_address,
            } => {
                let (final_controller_index, final_controller_address) =
                    resolve_controller_params(*controller_index, *controller_address)?;

                client
                    .disable_lp_token_controller(
                        store,
                        lp_token_mint,
                        final_controller_index,
                        final_controller_address,
                    )?
                    .into_bundle_with_options(options)?
            }
            #[cfg(feature = "execute")]
            Command::Stake {
                kind,
                lp_token_mint,
                amount,
                position_id,
                controller_index,
                controller_address,
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

                let (final_controller_index, final_controller_address) =
                    resolve_controller_params(*controller_index, *controller_address)?;

                // Create stake builder with controller parameters
                let stake_params = StakeLpTokenParams {
                    store,
                    lp_token_kind: *kind,
                    lp_token_mint,
                    oracle,
                    amount: stake_amount,
                    controller_index: final_controller_index,
                    controller_address: final_controller_address,
                };

                let builder = match position_id {
                    Some(pos_id) => client
                        .stake_lp_token(stake_params.clone())
                        .with_position_id(*pos_id),
                    None => client.stake_lp_token(stake_params),
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
                controller_index,
                controller_address,
            } => {
                let (final_controller_index, final_controller_address) =
                    resolve_controller_params(*controller_index, *controller_address)?;
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
                let unstake_params = UnstakeLpTokenParams {
                    store,
                    lp_token_kind: *kind,
                    lp_token_mint,
                    position_id: *position_id,
                    unstake_amount: amount_u64,
                    controller_index: final_controller_index,
                    controller_address: final_controller_address,
                };
                let unstake_tx = client.unstake_lp_token(unstake_params)?;

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
                controller_index,
                controller_address,
            } => {
                let (final_controller_index, final_controller_address) =
                    resolve_controller_params(*controller_index, *controller_address)?;

                // Use provided owner
                let position_owner = *owner;

                // Calculate GT reward using direct core calculation logic
                let params = GtRewardCalculationParams {
                    store,
                    lp_token_mint,
                    owner: &position_owner,
                    position_id: *position_id,
                    controller_index: final_controller_index,
                    controller_address: final_controller_address.as_ref(),
                };
                let gt_reward_raw = client.calculate_gt_reward(params).await?;

                // Get GT decimals for proper display formatting
                let store_account = client
                    .account::<ZeroCopy<Store>>(store)
                    .await?
                    .ok_or_else(|| eyre::eyre!("Store not found"))?;
                let gt_decimals = store_account.0.gt.decimals;

                // Display the calculated GT reward
                println!("Calculated GT reward for position {position_id}:");
                println!("Owner: {position_owner}");
                println!("LP Token Mint: {lp_token_mint}");
                println!("GT Reward (raw units): {gt_reward_raw}");

                // Convert to human-readable GT amount using actual decimals
                let divisor = 10_u128.pow(gt_decimals as u32) as f64;
                let gt_amount_readable = gt_reward_raw as f64 / divisor;
                // Use actual GT decimals for calculation display, but limit to reasonable precision
                let calculation_precision = gt_decimals.min(8) as usize; // Max 8 decimal places for readability
                println!("GT Reward (readable): {gt_amount_readable:.calculation_precision$} GT");

                return Ok(());
            }
            Command::ClaimGt {
                lp_token_mint,
                position_id,
                controller_index,
                controller_address,
            } => {
                let (final_controller_index, final_controller_address) =
                    resolve_controller_params(*controller_index, *controller_address)?;

                // Prepare GT user account (idempotent operation) - same as Unstake
                let prepare_user = client.prepare_user(store)?;

                // Create claim GT reward transaction using SDK
                let claim_tx = client.claim_gt_reward(
                    store,
                    lp_token_mint,
                    *position_id,
                    final_controller_index,
                    final_controller_address,
                )?;

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
            Command::QueryPositions {
                owner,
                position_id,
                lp_token_mint,
                controller_index,
                controller_address,
            } => {
                // Get GT decimals for proper formatting
                let store_account = client
                    .account::<ZeroCopy<Store>>(store)
                    .await?
                    .ok_or_else(|| eyre::eyre!("Store not found"))?;
                let gt_decimals = store_account.0.gt.decimals;
                let output = &ctx.config().output();

                match (owner, position_id, lp_token_mint) {
                    // Mode 1: Query my positions (QueryMyPositions)
                    (None, None, None) => {
                        let positions = client.get_my_lp_positions(store).await?;
                        self.display_positions_list(&positions, output, gt_decimals)?;
                    }

                    // Mode 2: Query positions for specified owner (QueryPositions)
                    (Some(owner), None, None) => {
                        let positions = client.get_lp_positions(store, owner).await?;
                        self.display_positions_list(&positions, output, gt_decimals)?;
                    }

                    // Mode 3: Query all positions for owner and LP token mint
                    (Some(owner), None, Some(lp_token_mint)) => {
                        // Get all positions for the owner, then filter by LP token mint
                        let all_positions = client.get_lp_positions(store, owner).await?;
                        let filtered_positions: Vec<_> = all_positions
                            .into_iter()
                            .filter(|pos| pos.lp_token_mint.0 == *lp_token_mint)
                            .collect();

                        if filtered_positions.is_empty() {
                            println!(
                                "No positions found for owner={owner} and lp_token_mint={lp_token_mint}"
                            );
                        } else {
                            self.display_positions_list(&filtered_positions, output, gt_decimals)?;
                        }
                    }

                    // Mode 4: Query specific position (QueryPosition)
                    (Some(owner), Some(position_id), Some(lp_token_mint)) => {
                        let (final_controller_index, final_controller_address) =
                            resolve_controller_params(*controller_index, *controller_address)?;

                        let params = LpPositionQueryParams {
                            store,
                            owner,
                            position_id: *position_id,
                            lp_token_mint,
                            controller_index: final_controller_index,
                            controller_address: final_controller_address.as_ref(),
                        };
                        let position = client.get_lp_position(params).await?;

                        match position {
                            Some(pos) => {
                                self.display_single_position(&pos, output, gt_decimals)?;
                            }
                            None => {
                                println!(
                                    "Position not found: owner={owner}, position_id={position_id}, lp_token_mint={lp_token_mint}"
                                );
                            }
                        }
                    }

                    // Invalid parameter combination
                    _ => {
                        return Err(eyre::eyre!(
                            "Invalid parameter combination. Use:\n\
                            - No parameters: query my positions\n\
                            - --owner only: query owner's positions\n\
                            - --owner --lp-token-mint: query owner's positions for specific LP token\n\
                            - --owner --position-id --lp-token-mint: query specific position"
                        ));
                    }
                }
                return Ok(());
            }
            Command::QueryControllers { lp_token_mint } => {
                // Query controllers for the specified LP token mint or all controllers
                let controllers = match lp_token_mint {
                    Some(mint) => client.get_lp_controllers(mint).await?,
                    None => client.get_all_lp_controllers().await?,
                };

                let output = &ctx.config().output();
                self.display_controllers(&controllers, lp_token_mint.as_ref(), output)?;
                return Ok(());
            }
            Command::QueryGlobalState => {
                // Query LP global state using the client
                let global_state = client.get_lp_global_state().await?;

                let output = &ctx.config().output();
                self.display_global_state(&global_state, output, &ctx)?;
                return Ok(());
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

impl Lp {
    /// Display a list of LP staking positions.
    /// Table format: LP token, amount, staked time, APY, claimable GT (calculated)
    fn display_positions_list(
        &self,
        positions: &[gmsol_sdk::serde::serde_lp_position::SerdeLpStakingPosition],
        output: &crate::config::OutputFormat,
        gt_decimals: u8,
    ) -> eyre::Result<()> {
        if positions.is_empty() {
            println!("No LP staking positions found.");
            return Ok(());
        }

        // Create formatted positions with proper decimal formatting
        let formatted_positions: Vec<_> = positions
            .iter()
            .map(|pos| {
                let mut formatted = serde_json::to_value(pos).unwrap();
                if let Some(obj) = formatted.as_object_mut() {
                    // Format APY to configured decimal places
                    if let Some(apy_value) = obj.get("current_apy") {
                        if let Some(apy_str) = apy_value.as_str() {
                            if let Ok(apy_num) = apy_str.parse::<f64>() {
                                obj.insert(
                                    "current_apy".to_string(),
                                    serde_json::Value::String(format!(
                                        "{apy_num:.APY_DISPLAY_DECIMALS$}"
                                    )),
                                );
                            }
                        }
                    }
                    // Format Average APY to configured decimal places
                    if let Some(apy_value) = obj.get("average_apy") {
                        if let Some(apy_str) = apy_value.as_str() {
                            if let Ok(apy_num) = apy_str.parse::<f64>() {
                                obj.insert(
                                    "average_apy".to_string(),
                                    serde_json::Value::String(format!(
                                        "{apy_num:.APY_DISPLAY_DECIMALS$}"
                                    )),
                                );
                            }
                        }
                    }
                    // Format GT using dynamic decimals from store
                    if let Some(gt_value) = obj.get("claimable_gt") {
                        if let Some(gt_str) = gt_value.as_str() {
                            if let Ok(gt_num) = gt_str.parse::<f64>() {
                                // Use the actual GT decimals from store, but limit to reasonable display precision
                                let display_precision =
                                    gt_decimals.min(GT_DISPLAY_DECIMALS as u8) as usize;
                                obj.insert(
                                    "claimable_gt".to_string(),
                                    serde_json::Value::String(format!(
                                        "{gt_num:.display_precision$}"
                                    )),
                                );
                            }
                        }
                    }
                    // Format stake start time to human readable format
                    if let Some(time_value) = obj.get("stake_start_time") {
                        if let Some(time_num) = time_value.as_i64() {
                            let formatted_time = self.format_timestamp(time_num);
                            obj.insert(
                                "stake_start_time".to_string(),
                                serde_json::Value::String(formatted_time),
                            );
                        }
                    }
                }
                formatted
            })
            .collect();

        let options = DisplayOptions::table_projection([
            ("position_id", "Position ID"),
            ("lp_token_symbol", "LP Token"),
            ("controller_index", "Controller Index"),
            ("controller", "Controller Address"),
            ("staked_amount", "Amount"),
            ("stake_start_time", "Staked Time"),
            ("current_apy", "Current APY"),
            ("average_apy", "Average APY"),
            ("claimable_gt", "Claimable GT"),
        ])
        .set_empty_message("No LP staking positions found.");

        println!("{}", output.display_many(formatted_positions, options)?);
        Ok(())
    }

    /// Display a single LP staking position with detailed information.
    /// Single format: LP token, amount, staked time, APY, claimable GT (calculated)
    fn display_single_position(
        &self,
        position: &gmsol_sdk::serde::serde_lp_position::SerdeLpStakingPosition,
        output: &crate::config::OutputFormat,
        gt_decimals: u8,
    ) -> eyre::Result<()> {
        // Format single position with proper decimal formatting
        let mut formatted = serde_json::to_value(position).unwrap();
        if let Some(obj) = formatted.as_object_mut() {
            // Format APY to configured decimal places
            if let Some(apy_value) = obj.get("current_apy") {
                if let Some(apy_str) = apy_value.as_str() {
                    if let Ok(apy_num) = apy_str.parse::<f64>() {
                        obj.insert(
                            "current_apy".to_string(),
                            serde_json::Value::String(format!("{apy_num:.APY_DISPLAY_DECIMALS$}")),
                        );
                    }
                }
            }
            // Format Average APY to configured decimal places
            if let Some(apy_value) = obj.get("average_apy") {
                if let Some(apy_str) = apy_value.as_str() {
                    if let Ok(apy_num) = apy_str.parse::<f64>() {
                        obj.insert(
                            "average_apy".to_string(),
                            serde_json::Value::String(format!("{apy_num:.APY_DISPLAY_DECIMALS$}")),
                        );
                    }
                }
            }
            // Format GT using dynamic decimals from store
            if let Some(gt_value) = obj.get("claimable_gt") {
                if let Some(gt_str) = gt_value.as_str() {
                    if let Ok(gt_num) = gt_str.parse::<f64>() {
                        // Use the actual GT decimals from store, but limit to reasonable display precision
                        let display_precision = gt_decimals.min(GT_DISPLAY_DECIMALS as u8) as usize;
                        obj.insert(
                            "claimable_gt".to_string(),
                            serde_json::Value::String(format!("{gt_num:.display_precision$}")),
                        );
                    }
                }
            }
            // Format stake start time to human readable format
            if let Some(time_value) = obj.get("stake_start_time") {
                if let Some(time_num) = time_value.as_i64() {
                    let formatted_time = self.format_timestamp(time_num);
                    obj.insert(
                        "stake_start_time".to_string(),
                        serde_json::Value::String(formatted_time),
                    );
                }
            }
        }

        // Single position display: Position ID, LP token, controller info, amount, staked time, APY, claimable GT
        let options = DisplayOptions::table_projection([
            ("position_id", "Position ID"),
            ("lp_token_symbol", "LP Token"),
            ("controller_index", "Controller Index"),
            ("controller", "Controller Address"),
            ("staked_amount", "Amount"),
            ("stake_start_time", "Staked Time"),
            ("current_apy", "Current APY"),
            ("average_apy", "Average APY"),
            ("claimable_gt", "Claimable GT"),
        ]);

        // For single position, display as single item
        println!(
            "{}",
            output.display_many(std::iter::once(formatted), options)?
        );
        Ok(())
    }

    /// Display LP controllers for a specific token mint or all controllers.
    fn display_controllers(
        &self,
        controllers: &[gmsol_sdk::serde::serde_lp_controller::SerdeLpController],
        lp_token_mint: Option<&Pubkey>,
        output: &crate::config::OutputFormat,
    ) -> eyre::Result<()> {
        if controllers.is_empty() {
            match lp_token_mint {
                Some(mint) => println!("No controllers found for LP token: {mint}"),
                None => println!("No LP token controllers found."),
            }
            return Ok(());
        }

        match lp_token_mint {
            Some(mint) => println!("Controllers for LP token: {mint}"),
            None => println!("All LP token controllers:"),
        }

        let options = DisplayOptions::table_projection([
            ("lp_token_mint", "LP Token Mint"),
            ("controller_index", "Index"),
            ("controller", "Controller Address"),
            ("is_enabled", "Enabled"),
            ("total_positions", "Total Positions"),
            ("disabled_at", "Disabled At"),
            ("disabled_cum_inv_cost", "Disabled Cum Inv Cost"),
        ])
        .set_empty_message("No controllers found.");

        println!("{}", output.display_many(controllers, options)?);
        Ok(())
    }

    /// Display LP Global State information.
    fn display_global_state(
        &self,
        global_state: &gmsol_sdk::serde::serde_lp_global_state::SerdeLpGlobalState,
        output: &crate::config::OutputFormat,
        ctx: &super::Context<'_>,
    ) -> eyre::Result<()> {
        println!("LP Global State Information:");

        let options = DisplayOptions::table_projection([("field", "Field"), ("value", "Value")]);

        let mut state_data = Vec::new();

        // Calculate and display Global State PDA address
        let client = ctx.client()?;
        let lp_program = client.lp_program_for_builders();
        let global_state_address = lp_program.find_global_state_address();

        state_data.push(serde_json::json!({
            "field": "LP Program ID",
            "value": lp_program.id.to_string()
        }));

        state_data.push(serde_json::json!({
            "field": "Global State Address (PDA)",
            "value": global_state_address.to_string()
        }));

        // Basic information
        state_data.push(serde_json::json!({
            "field": "Authority",
            "value": global_state.authority.to_string()
        }));

        state_data.push(serde_json::json!({
            "field": "Pending Authority",
            "value": global_state.pending_authority.to_string()
        }));

        state_data.push(serde_json::json!({
            "field": "Min Stake Value (1e20)",
            "value": global_state.min_stake_value.to_string()
        }));

        state_data.push(serde_json::json!({
            "field": "Claim Enabled",
            "value": global_state.claim_enabled
        }));

        state_data.push(serde_json::json!({
            "field": "Pricing Staleness (seconds)",
            "value": global_state.pricing_staleness_seconds
        }));

        state_data.push(serde_json::json!({
            "field": "PDA Bump",
            "value": global_state.bump
        }));

        // APY Gradient complete list (all buckets from 0 to end)
        if !global_state.apy_gradient.is_empty() {
            for (index, apy_value) in global_state.apy_gradient.iter().enumerate() {
                state_data.push(serde_json::json!({
                    "field": format!("APY Gradient [{index}] (1e20)"),
                    "value": apy_value.to_string()
                }));
            }

            state_data.push(serde_json::json!({
                "field": "Total APY Buckets",
                "value": global_state.apy_gradient.len()
            }));
        }

        println!("{}", output.display_many(state_data, options)?);
        Ok(())
    }

    /// Format Unix timestamp to human readable date and time string.
    /// Returns format: "YYYY-MM-DD HH:MM:SS UTC" or "Invalid timestamp" for invalid values.
    fn format_timestamp(&self, timestamp: i64) -> String {
        if timestamp <= 0 {
            return "Invalid timestamp".to_string();
        }

        match time::OffsetDateTime::from_unix_timestamp(timestamp) {
            Ok(datetime) => {
                // Format as "YYYY-MM-DD HH:MM:SS UTC"
                match datetime.format(
                    &time::format_description::parse(
                        "[year]-[month]-[day] [hour]:[minute]:[second] UTC",
                    )
                    .unwrap(),
                ) {
                    Ok(formatted) => formatted,
                    Err(_) => "Invalid timestamp".to_string(),
                }
            }
            Err(_) => "Invalid timestamp".to_string(),
        }
    }
}
