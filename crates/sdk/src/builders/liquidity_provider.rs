use std::{collections::BTreeSet, num::NonZeroU64};

use anchor_lang::system_program;
use gmsol_programs::gmsol_liquidity_provider::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, Program, ProgramExt};
use gmsol_utils::{oracle::PriceProviderKind, token_config::TokensWithFeed};

#[cfg(feature = "client")]
use gmsol_model::utils::apply_factor;
#[cfg(feature = "client")]
use gmsol_programs::{
    anchor_lang::Discriminator,
    gmsol_store::{
        accounts::{Glv, Market, Store},
        constants::MARKET_DECIMALS,
    },
};
#[cfg(feature = "client")]
use gmsol_solana_utils::client_traits::{FromRpcClientWith, RpcClientExt};
#[cfg(feature = "client")]
use gmsol_utils::{pubkey::optional_address, swap::SwapActionParams, token_config::token_records};
use rand::Rng;
#[cfg(feature = "client")]
use solana_client::{
    rpc_config::RpcAccountInfoConfig,
    rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use typed_builder::TypedBuilder;

#[cfg(feature = "client")]
use crate::client::accounts::{get_program_accounts_with_context, ProgramAccountsConfigForRpc};

use crate::{
    serde::{
        serde_price_feed::{to_tokens_with_feeds, SerdeTokenRecord},
        StringPubkey,
    },
    utils::{
        glv::split_to_accounts,
        token_map::{FeedAddressMap, FeedsParser},
    },
};

#[cfg(feature = "client")]
use crate::{
    serde::serde_lp_position::{
        fallback_lp_token_symbol, LpPositionComputedData, SerdeLpStakingPosition,
    },
    utils::{market::ordered_tokens, token_map::TokenMap, zero_copy::ZeroCopy},
};

use super::StoreProgram;

// ============================================================================
// Parameter Structs
// ============================================================================

/// Parameters for querying a specific LP position.
#[derive(Debug, Clone)]
pub struct LpPositionQueryParams<'a> {
    /// Store address
    pub store: &'a Pubkey,
    /// Owner of the position
    pub owner: &'a Pubkey,
    /// Position ID
    pub position_id: u64,
    /// LP token mint address
    pub lp_token_mint: &'a Pubkey,
    /// Controller index
    pub controller_index: u64,
    /// Optional controller address (takes precedence over controller_index)
    pub controller_address: Option<&'a Pubkey>,
}

/// Parameters for calculating GT rewards.
#[derive(Debug, Clone)]
pub struct GtRewardCalculationParams<'a> {
    /// Store address
    pub store: &'a Pubkey,
    /// LP token mint address
    pub lp_token_mint: &'a Pubkey,
    /// Owner of the position
    pub owner: &'a Pubkey,
    /// Position ID
    pub position_id: u64,
    /// Controller index
    pub controller_index: u64,
    /// Optional controller address (takes precedence over controller_index)
    pub controller_address: Option<&'a Pubkey>,
}

/// Parameters for staking LP tokens.
#[derive(Debug, Clone)]
pub struct StakeLpTokenParams<'a> {
    /// Store address
    pub store: &'a Pubkey,
    /// LP token kind (GM or GLV)
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address
    pub lp_token_mint: &'a Pubkey,
    /// Oracle buffer account
    pub oracle: &'a Pubkey,
    /// Stake amount
    pub amount: std::num::NonZeroU64,
    /// Controller index
    pub controller_index: u64,
    /// Optional controller address (takes precedence over controller_index)
    pub controller_address: Option<Pubkey>,
}

/// Parameters for unstaking LP tokens.
#[derive(Debug, Clone)]
pub struct UnstakeLpTokenParams<'a> {
    /// Store address
    pub store: &'a Pubkey,
    /// LP token kind (GM or GLV)
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address
    pub lp_token_mint: &'a Pubkey,
    /// Position ID
    pub position_id: u64,
    /// Unstake amount
    pub unstake_amount: u64,
    /// Controller index
    pub controller_index: u64,
    /// Optional controller address (takes precedence over controller_index)
    pub controller_address: Option<Pubkey>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Resolves controller address from optional controller_address and controller_index.
///
/// # Arguments
/// * `lp_program` - The liquidity provider program instance
/// * `global_state` - Global state address
/// * `lp_token_mint` - LP token mint address  
/// * `controller_index` - Controller index
/// * `controller_address` - Optional controller address
///
/// # Returns
/// Controller address (controller_address takes precedence if provided)
fn resolve_controller_address(
    lp_program: &LiquidityProviderProgram,
    global_state: &Pubkey,
    lp_token_mint: &Pubkey,
    controller_index: u64,
    controller_address: Option<&Pubkey>,
) -> Pubkey {
    if let Some(addr) = controller_address {
        *addr
    } else {
        lp_program.find_lp_token_controller_address(global_state, lp_token_mint, controller_index)
    }
}

/// Resolves controller address for builders (with StringPubkey types).
///
/// # Arguments
/// * `lp_program` - The liquidity provider program instance
/// * `global_state` - Global state address
/// * `lp_token_mint` - LP token mint address  
/// * `controller_index` - Controller index
/// * `controller_address` - Optional controller address (StringPubkey)
///
/// # Returns
/// Controller address (controller_address takes precedence if provided)
fn resolve_controller_address_for_builder(
    lp_program: &LiquidityProviderProgram,
    global_state: &Pubkey,
    lp_token_mint: &Pubkey,
    controller_index: u64,
    controller_address: Option<&crate::serde::StringPubkey>,
) -> Pubkey {
    if let Some(addr) = controller_address {
        addr.0
    } else {
        lp_program.find_lp_token_controller_address(global_state, lp_token_mint, controller_index)
    }
}

// ============================================================================
// Constants
// ============================================================================

/// Seconds per week for APY gradient calculations (7 * 24 * 3600)
const SECONDS_PER_WEEK: u128 = 7 * 24 * 3600;

/// Last index of APY buckets (APY_BUCKETS - 1 = 53 - 1 = 52)
const APY_LAST_INDEX: usize = 52;

#[cfg(feature = "client")]
/// Seconds per year for APY calculations (365.25 * 24 * 3600)
const SECONDS_PER_YEAR: u128 = 31_557_600;

// ============================================================================
// Structs and Implementations
// ============================================================================

/// A liquidity-provider program.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct LiquidityProviderProgram {
    /// Program ID.
    #[builder(setter(into))]
    pub id: StringPubkey,
}

impl Default for LiquidityProviderProgram {
    fn default() -> Self {
        Self {
            id: <Self as anchor_lang::Id>::id().into(),
        }
    }
}

impl anchor_lang::Id for LiquidityProviderProgram {
    fn id() -> Pubkey {
        gmsol_programs::gmsol_liquidity_provider::ID
    }
}

impl Program for LiquidityProviderProgram {
    fn id(&self) -> &Pubkey {
        &self.id
    }
}

impl LiquidityProviderProgram {
    /// Find PDA for global state account.
    pub fn find_global_state_address(&self) -> Pubkey {
        crate::pda::find_lp_global_state_address(&self.id).0
    }

    /// Query all LP staking positions for a specific owner (builder layer implementation)
    #[cfg(feature = "client")]
    pub async fn query_lp_positions(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
        store: &Pubkey,
        owner: &Pubkey,
    ) -> crate::Result<Vec<crate::serde::serde_lp_position::SerdeLpStakingPosition>> {
        // Get global state and store data
        let global_state_address = self.find_global_state_address();
        let global_state = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState>(
                &global_state_address,
                Default::default(),
            )
            .await?;

        let store_account = client
            .get_anchor_account::<crate::utils::zero_copy::ZeroCopy<gmsol_programs::gmsol_store::accounts::Store>>(store, Default::default())
            .await?;
        let gt_decimals = store_account.0.gt.decimals;

        // Query all Position accounts for this owner
        let config = ProgramAccountsConfigForRpc {
            filters: Some(vec![
                RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                    0,
                    gmsol_programs::gmsol_liquidity_provider::accounts::Position::DISCRIMINATOR,
                )),
                RpcFilterType::Memcmp(Memcmp::new(
                    8,
                    MemcmpEncodedBytes::Base58(owner.to_string()),
                )),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                ..RpcAccountInfoConfig::default()
            },
        };

        let position_accounts_result =
            get_program_accounts_with_context(client, &self.id, config).await?;
        let position_accounts = position_accounts_result.into_value();

        let mut results = Vec::new();

        for (_position_address, account) in position_accounts {
            // Deserialize position account
            let position: gmsol_programs::gmsol_liquidity_provider::accounts::Position =
                anchor_lang::AccountDeserialize::try_deserialize(&mut account.data.as_slice())
                    .map_err(|e| {
                        crate::Error::custom(format!("Failed to deserialize position: {e}"))
                    })?;

            // Get controller for this position
            let controller = client
                .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController>(&position.controller, Default::default())
                .await?;

            // Calculate actual GT reward for this position (using pre-fetched store data)
            let params = GtRewardCalculationParams {
                store,
                lp_token_mint: &position.lp_mint,
                owner: &position.owner,
                position_id: position.position_id,
                controller_index: controller.controller_index,
                controller_address: Some(&position.controller),
            };
            let actual_gt_reward = self
                .calculate_gt_reward_with_store(client, &params, &store_account.0)
                .await?;

            // Use builder to create serde position with actual GT reward
            let serde_position = Self::create_serde_position(
                &position,
                &controller,
                &global_state,
                gt_decimals,
                actual_gt_reward,
            )?;

            results.push(serde_position);
        }

        Ok(results)
    }

    /// Query a specific LP staking position (builder layer implementation)
    #[cfg(feature = "client")]
    pub async fn query_lp_position(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
        params: &LpPositionQueryParams<'_>,
    ) -> crate::Result<Option<crate::serde::serde_lp_position::SerdeLpStakingPosition>> {
        // Get global state and addresses
        let global_state_address = self.find_global_state_address();

        let controller_addr = resolve_controller_address(
            self,
            &global_state_address,
            params.lp_token_mint,
            params.controller_index,
            params.controller_address,
        );

        let position_address =
            self.find_stake_position_address(params.owner, params.position_id, &controller_addr);

        // Get accounts
        let global_state = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState>(
                &global_state_address,
                Default::default(),
            )
            .await?;

        let store_account = client
            .get_anchor_account::<crate::utils::zero_copy::ZeroCopy<gmsol_programs::gmsol_store::accounts::Store>>(params.store, Default::default())
            .await?;
        let gt_decimals = store_account.0.gt.decimals;

        // Try to get position account - if it doesn't exist, return None
        let position = match client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::Position>(
                &position_address,
                Default::default(),
            )
            .await
        {
            Ok(pos) => pos,
            Err(_) => return Ok(None), // Position not found
        };

        // Get controller
        let controller = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController>(&controller_addr, Default::default())
            .await?;

        // Calculate actual GT reward for this position (using pre-fetched store data)
        let gt_params = GtRewardCalculationParams {
            store: params.store,
            lp_token_mint: params.lp_token_mint,
            owner: params.owner,
            position_id: params.position_id,
            controller_index: params.controller_index,
            controller_address: params.controller_address,
        };
        let actual_gt_reward = self
            .calculate_gt_reward_with_store(client, &gt_params, &store_account.0)
            .await?;

        // Use builder to create serde position with actual GT reward
        let serde_position = Self::create_serde_position(
            &position,
            &controller,
            &global_state,
            gt_decimals,
            actual_gt_reward,
        )?;

        Ok(Some(serde_position))
    }

    /// Query all LP controllers for a specific token mint (builder layer implementation)
    #[cfg(feature = "client")]
    pub async fn query_lp_controllers(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
        lp_token_mint: &Pubkey,
    ) -> crate::Result<Vec<crate::serde::serde_lp_controller::SerdeLpController>> {
        // First, try to query all LpTokenController accounts to see if any exist
        let all_controllers_config = ProgramAccountsConfigForRpc {
            filters: Some(vec![
                RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                    0,
                    gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController::DISCRIMINATOR,
                )),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                ..RpcAccountInfoConfig::default()
            },
        };

        let all_controller_accounts_result =
            get_program_accounts_with_context(client, &self.id, all_controllers_config).await?;
        let all_controller_accounts = all_controller_accounts_result.into_value();

        let mut results = Vec::new();

        for (controller_address, account) in all_controller_accounts {
            // Deserialize controller account
            let controller: gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController =
                anchor_lang::AccountDeserialize::try_deserialize(&mut account.data.as_slice())
                    .map_err(|e| {
                        crate::Error::custom(format!("Failed to deserialize controller: {e}"))
                    })?;

            // Check if this controller matches our target lp_token_mint
            if controller.lp_token_mint == *lp_token_mint {
                let serde_controller =
                    crate::serde::serde_lp_controller::SerdeLpController::from_controller(
                        &controller,
                        &controller_address,
                    );
                results.push(serde_controller);
            }
        }

        Ok(results)
    }

    /// Query LP Global State (builder layer implementation)
    #[cfg(feature = "client")]
    pub async fn query_lp_global_state(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> crate::Result<crate::serde::serde_lp_global_state::SerdeLpGlobalState> {
        let global_state_address = self.find_global_state_address();

        let global_state = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState>(
                &global_state_address,
                Default::default(),
            )
            .await
            .map_err(crate::Error::from)?;

        Ok(
            crate::serde::serde_lp_global_state::SerdeLpGlobalState::from_global_state(
                &global_state,
            ),
        )
    }

    /// Calculate GT reward for a specific position (builder layer implementation)
    /// This implements the same calculation as compute_reward_with_cpi in lib.rs
    #[cfg(feature = "client")]
    pub async fn calculate_gt_reward(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
        params: &GtRewardCalculationParams<'_>,
    ) -> crate::Result<u128> {
        // Get store account
        let store_account = client
            .get_anchor_account::<crate::utils::zero_copy::ZeroCopy<gmsol_programs::gmsol_store::accounts::Store>>(params.store, Default::default())
            .await?;

        self.calculate_gt_reward_with_store(client, params, &store_account.0)
            .await
    }

    /// Calculate GT reward for a specific position with pre-fetched store data (optimized version)
    /// This implements the same calculation as compute_reward_with_cpi in lib.rs
    #[cfg(feature = "client")]
    pub async fn calculate_gt_reward_with_store(
        &self,
        client: &solana_client::nonblocking::rpc_client::RpcClient,
        params: &GtRewardCalculationParams<'_>,
        store_account: &gmsol_programs::gmsol_store::accounts::Store,
    ) -> crate::Result<u128> {
        // Get required accounts for GT calculation (store account already provided)
        let global_state_address = self.find_global_state_address();

        let controller_addr = resolve_controller_address(
            self,
            &global_state_address,
            params.lp_token_mint,
            params.controller_index,
            params.controller_address,
        );

        let position_address =
            self.find_stake_position_address(params.owner, params.position_id, &controller_addr);

        // Get other required accounts (store is already provided)
        let global_state = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState>(
                &global_state_address,
                Default::default(),
            )
            .await?;

        let position = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::Position>(
                &position_address,
                Default::default(),
            )
            .await?;

        let controller = client
            .get_anchor_account::<gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController>(&controller_addr, Default::default())
            .await?;

        // GT reward calculation using precise on-chain logic
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Step 1: Get current cumulative inverse cost factor (mirrors compute_reward_with_cpi)
        let (cum_now, effective_end_time) = if !controller.is_enabled {
            // Controller is disabled, use disabled snapshot values
            (controller.disabled_cum_inv_cost, controller.disabled_at)
        } else {
            // Controller is enabled, calculate current cumulative inverse cost factor
            // using GT state data: cum_now = store.gt.cumulative_inv_cost_factor + (current_time - last_update_time) * (1 / current_minting_cost)

            // This mirrors the exact calculation in gt.rs:update_cumulative_inv_cost_factor()
            let gt_state = &store_account.gt;

            // Get current values from GT state
            let last_update_time = gt_state.last_cumulative_inv_cost_factor_ts;
            let current_cumulative = gt_state.cumulative_inv_cost_factor;
            let current_minting_cost = gt_state.minting_cost;

            // Calculate time difference since last update
            let duration_since_update = current_time.saturating_sub(last_update_time);

            let updated_cumulative = if duration_since_update > 0 {
                let duration_value = duration_since_update as u128;

                // Calculate delta: (duration_seconds * MARKET_USD_UNIT) / minting_cost
                // This exactly mirrors the div_to_factor calculation in gt.rs:update_cumulative_inv_cost_factor
                let market_usd_unit = 10u128.pow(MARKET_DECIMALS as u32); // MARKET_USD_UNIT
                let delta = if current_minting_cost > 0 {
                    (duration_value * market_usd_unit) / current_minting_cost
                } else {
                    0 // Prevent division by zero
                };

                current_cumulative.saturating_add(delta)
            } else {
                current_cumulative
            };

            (updated_cumulative, current_time)
        };

        // Step 2: Calculate inv_cost_integral (mirrors lib.rs line 691)
        let prev_cum = position.cum_inv_cost;
        if cum_now < prev_cum {
            return Ok(0); // Sanity check: cumulative factor should be monotonically increasing
        }
        let inv_cost_integral = cum_now - prev_cum;

        // Step 3: Calculate duration and time-weighted APY (mirrors lib.rs lines 694-704)
        let duration_seconds = effective_end_time.saturating_sub(position.stake_start_time);
        if duration_seconds <= 0 {
            return Ok(0);
        }

        let avg_apy = Self::compute_time_weighted_apy(
            position.stake_start_time,
            effective_end_time,
            &global_state.apy_gradient,
        );

        // Convert to per-second APY (exactly matches lib.rs lines 700-704)
        let avg_apy_per_sec = if SECONDS_PER_YEAR > 0 {
            avg_apy / SECONDS_PER_YEAR
        } else {
            0
        };

        // Step 4: Calculate GT reward using exact on-chain formula (mirrors calculate_gt_reward_amount)

        // Convert notional USD to per-second notionals using APY per second (lib.rs line 639)
        let per_sec_factor =
            apply_factor::<u128, MARKET_DECIMALS>(&position.staked_value_usd, &avg_apy_per_sec)
                .ok_or_else(|| {
                    crate::Error::custom("Math overflow in per_sec_factor calculation")
                })?;

        // Apply the integral of MARKET_USD_UNIT / price(t) over time (lib.rs line 643)
        let gt_raw = apply_factor::<u128, MARKET_DECIMALS>(&per_sec_factor, &inv_cost_integral)
            .ok_or_else(|| crate::Error::custom("Math overflow in gt_raw calculation"))?;

        // Cap at u64::MAX and return as u128 (lib.rs line 646)
        Ok(gt_raw.min(u64::MAX as u128))
    }

    /// Compute current display APY based on current staking duration (returns APY for current week).
    /// This is used for UI display purposes to show the APY rate for the current week.
    pub fn compute_current_display_apy(
        start_time: i64,
        end_time: i64,
        apy_gradient: &[u128; 53],
    ) -> u128 {
        if end_time <= start_time {
            return apy_gradient[0];
        }

        let total_seconds: u128 = (end_time - start_time) as u128;
        if total_seconds == 0 {
            return apy_gradient[0];
        }

        // Calculate which week we're in
        let week_index = total_seconds / SECONDS_PER_WEEK;

        // Get the corresponding APY from gradient array
        if week_index >= APY_LAST_INDEX as u128 {
            apy_gradient[APY_LAST_INDEX] // Use last bucket for weeks 52+
        } else {
            apy_gradient[week_index as usize]
        }
    }

    /// Compute time-weighted average APY over [start, end] using APY_BUCKETS-bucket weekly gradient (1e20-scaled).
    /// This mirrors the exact computation from the core program and is used for GT reward calculations.
    /// This implements the same calculation as compute_time_weighted_apy in lib.rs
    pub fn compute_time_weighted_apy(
        start_time: i64,
        end_time: i64,
        apy_gradient: &[u128; 53],
    ) -> u128 {
        if end_time <= start_time {
            return apy_gradient[0];
        }
        let total_seconds: u128 = (end_time - start_time) as u128;
        if total_seconds == 0 {
            return apy_gradient[0];
        }

        let full_weeks: u128 = total_seconds / SECONDS_PER_WEEK;
        let rem_seconds: u128 = total_seconds % SECONDS_PER_WEEK;

        // Sum full-week contributions (mirrors lib.rs:740-751)
        let mut acc: u128 = 0;
        let capped_full: u128 = full_weeks.min(APY_LAST_INDEX as u128);
        for &apy_value in apy_gradient.iter().take(capped_full as usize) {
            acc = acc.saturating_add(apy_value.saturating_mul(SECONDS_PER_WEEK));
        }
        if full_weeks > (APY_LAST_INDEX as u128) {
            let extra = full_weeks - (APY_LAST_INDEX as u128); // weeks APY_LAST_INDEX+ use bucket APY_LAST_INDEX
            acc = acc.saturating_add(
                apy_gradient[APY_LAST_INDEX].saturating_mul(SECONDS_PER_WEEK.saturating_mul(extra)),
            );
        }

        // Add partial-week remainder (mirrors lib.rs:754-757)
        if rem_seconds > 0 {
            let idx =
                usize::try_from(full_weeks.min(APY_LAST_INDEX as u128)).unwrap_or(APY_LAST_INDEX);
            acc = acc.saturating_add(apy_gradient[idx].saturating_mul(rem_seconds));
        }

        acc / total_seconds
    }

    /// Find PDA for stake position account.
    pub fn find_stake_position_address(
        &self,
        owner: &Pubkey,
        position_id: u64,
        controller: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_lp_stake_position_address(owner, position_id, controller, &self.id).0
    }

    /// Find PDA for stake position vault.
    pub fn find_stake_position_vault_address(&self, position: &Pubkey) -> Pubkey {
        crate::pda::find_lp_stake_position_vault_address(position, &self.id).0
    }

    /// Find PDA for LP token controller account.
    pub fn find_lp_token_controller_address(
        &self,
        global_state: &Pubkey,
        lp_token_mint: &Pubkey,
        controller_index: u64,
    ) -> Pubkey {
        crate::pda::find_lp_token_controller_address(
            global_state,
            lp_token_mint,
            controller_index,
            &self.id,
        )
        .0
    }

    /// Create serde position from raw position data (helper for ops layer)
    #[cfg(feature = "client")]
    pub fn create_serde_position(
        position: &gmsol_programs::gmsol_liquidity_provider::accounts::Position,
        controller: &gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController,
        global_state: &gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState,
        gt_decimals: u8,
        claimable_gt: u128,
    ) -> crate::Result<crate::serde::serde_lp_position::SerdeLpStakingPosition> {
        // Calculate current APY
        let computed_data = Self::compute_position_data(position, controller, global_state)?;

        // Get LP token symbol
        let lp_token_symbol = fallback_lp_token_symbol(&position.lp_mint.into());

        // Create computed data with symbol (use provided GT value)
        let computed_data_with_symbol = LpPositionComputedData {
            claimable_gt: crate::utils::Amount::from_u128(claimable_gt, gt_decimals).map_err(
                |_| crate::Error::custom("Claimable GT amount exceeds maximum representable value"),
            )?,
            current_apy: crate::utils::Value::from_u128(computed_data.current_apy),
            lp_token_symbol,
        };

        // Convert to serde format
        SerdeLpStakingPosition::from_position(position, controller, computed_data_with_symbol)
    }

    /// Compute position data (APY and GT rewards) - internal helper
    /// Note: GT rewards are set to 0 for display purposes. Use calculate_gt_reward() method for precise GT calculations.
    #[cfg(feature = "client")]
    fn compute_position_data(
        position: &gmsol_programs::gmsol_liquidity_provider::accounts::Position,
        controller: &gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController,
        global_state: &gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState,
    ) -> crate::Result<PositionComputedData> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Calculate effective end time based on controller status
        let effective_end_time = if controller.is_enabled {
            current_time
        } else {
            controller.disabled_at
        };

        // Calculate current display APY (shows current week's APY for UI display)
        let current_display_apy = Self::compute_current_display_apy(
            position.stake_start_time,
            effective_end_time,
            &global_state.apy_gradient,
        );

        Ok(PositionComputedData {
            current_apy: current_display_apy, // Use display APY for UI
        })
    }
}

/// Internal helper struct for computed position data
#[cfg(feature = "client")]
struct PositionComputedData {
    current_apy: u128,
}

/// Builder for LP token staking instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct StakeLpToken {
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Oracle buffer account.
    #[builder(setter(into))]
    pub oracle: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token kind.
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// LP token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub lp_token_account: Option<StringPubkey>,
    /// Position ID.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub position_id: Option<u64>,
    /// Stake amount.
    pub amount: NonZeroU64,
    /// Controller index.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub controller_index: u64,
    /// Controller address (if provided, takes precedence over controller_index).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub controller_address: Option<StringPubkey>,
    /// Feeds Parser.
    #[cfg_attr(serde, serde(skip))]
    #[builder(default)]
    pub feeds_parser: FeedsParser,
}

impl StakeLpToken {
    /// Insert a feed parser.
    pub fn insert_feed_parser(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }

    /// Set a specific position ID instead of using random generation.
    pub fn with_position_id(mut self, position_id: u64) -> Self {
        self.position_id = Some(position_id);
        self
    }

    fn position_id(&self) -> u64 {
        self.position_id.unwrap_or_else(|| rand::thread_rng().gen())
    }

    fn lp_token_account(&self, token_program_id: &Pubkey) -> Pubkey {
        self.lp_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                anchor_spl::associated_token::get_associated_token_address_with_program_id(
                    &self.payer,
                    &self.lp_token_mint,
                    token_program_id,
                )
            })
    }

    fn shared_args(&self) -> SharedArgs {
        let owner = self.payer.0;
        let position_id = self.position_id();
        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        let position =
            self.lp_program
                .find_stake_position_address(&owner, position_id, &controller);
        let position_vault = self.lp_program.find_stake_position_vault_address(&position);

        SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store: self.store_program.store.0,
            gt_program: *self.store_program.id(),
        }
    }

    fn feeds(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Vec<AccountMeta>> {
        self.feeds_parser
            .parse(&hint.to_tokens_with_feeds()?)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)
    }

    fn stake_gm(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Instruction> {
        let SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_args();
        let token_program_id = anchor_spl::token::ID;
        let market = self.store_program.find_market_address(&lp_mint);

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        Ok(self
            .lp_program
            .anchor_instruction(args::StakeGm {
                position_id,
                gm_staked_amount: self.amount.get(),
            })
            .anchor_accounts(
                accounts::StakeGm {
                    global_state,
                    controller,
                    lp_mint,
                    position,
                    position_vault,
                    gt_store,
                    gt_program,
                    owner,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    token_map: hint.token_map.0,
                    oracle: self.oracle.0,
                    market,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    event_authority: self.store_program.find_event_authority_address(),
                },
                false,
            )
            .accounts(self.feeds(hint)?)
            .build())
    }

    fn stake_glv(&self, hint: &StakeLpTokenHint) -> gmsol_solana_utils::Result<Instruction> {
        let SharedArgs {
            owner,
            position_id,
            global_state,
            lp_mint,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_args();
        let token_program_id = anchor_spl::token_2022::ID;
        let glv = self.store_program.find_glv_address(&lp_mint);
        let market_tokens = hint.glv_market_tokens.as_ref().ok_or_else(|| {
            gmsol_solana_utils::Error::custom("Hint must include the market token list for the GLV")
        })?;
        let glv_accounts = split_to_accounts(
            market_tokens.iter().map(|token| token.0),
            &glv,
            &gt_store,
            &gt_program,
            &token_program_id,
            false,
        )
        .0;

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        Ok(self
            .lp_program
            .anchor_instruction(args::StakeGlv {
                position_id,
                glv_staked_amount: self.amount.get(),
            })
            .anchor_accounts(
                accounts::StakeGlv {
                    global_state,
                    controller,
                    lp_mint,
                    position,
                    position_vault,
                    gt_store,
                    gt_program,
                    owner,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    token_map: hint.token_map.0,
                    oracle: self.oracle.0,
                    glv,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    event_authority: self.store_program.find_event_authority_address(),
                },
                false,
            )
            .accounts(glv_accounts)
            .accounts(self.feeds(hint)?)
            .build())
    }
}

impl IntoAtomicGroup for StakeLpToken {
    type Hint = StakeLpTokenHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let stake = match self.lp_token_kind {
            LpTokenKind::Gm => self.stake_gm(hint),
            LpTokenKind::Glv => self.stake_glv(hint),
        }?;

        insts.add(stake);

        Ok(insts)
    }
}

/// Hint for [`StakeLpToken`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct StakeLpTokenHint {
    /// Token map.
    #[builder(setter(into))]
    pub token_map: StringPubkey,
    /// Feeds.
    #[builder(setter(into))]
    pub feeds: Vec<SerdeTokenRecord>,
    /// Market tokens (GLV only).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub glv_market_tokens: Option<BTreeSet<StringPubkey>>,
}

impl StakeLpTokenHint {
    /// Create [`TokensWithFeed`].
    pub fn to_tokens_with_feeds(&self) -> gmsol_solana_utils::Result<TokensWithFeed> {
        to_tokens_with_feeds(&self.feeds).map_err(gmsol_solana_utils::Error::custom)
    }
}

#[cfg(feature = "client")]
impl FromRpcClientWith<StakeLpToken> for StakeLpTokenHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a StakeLpToken,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        let store_program = &builder.store_program;
        let store_address = &store_program.store.0;
        let store = client
            .get_anchor_account::<ZeroCopy<Store>>(store_address, Default::default())
            .await?
            .0;
        let token_map_address = optional_address(&store.token_map)
            .ok_or_else(|| gmsol_solana_utils::Error::custom("token map is not set"))?;

        let (tokens, glv_market_tokens) = match builder.lp_token_kind {
            LpTokenKind::Gm => {
                let market_address = store_program.find_market_address(&builder.lp_token_mint);
                let market = client
                    .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
                    .await?
                    .0;
                (ordered_tokens(&market.meta.into()), None)
            }
            LpTokenKind::Glv => {
                let glv_address = store_program.find_glv_address(&builder.lp_token_mint);
                let glv = client
                    .get_anchor_account::<ZeroCopy<Glv>>(&glv_address, Default::default())
                    .await?
                    .0;
                let mut collector = glv.tokens_collector(None::<&SwapActionParams>);
                for token in glv.market_tokens() {
                    let market_address = store_program.find_market_address(&token);
                    let market = client
                        .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
                        .await?
                        .0;
                    collector.insert_token(&market.meta.index_token_mint);
                }
                let market_tokens = glv.market_tokens().map(StringPubkey).collect();
                (collector.unique_tokens(), Some(market_tokens))
            }
        };

        let token_map = client
            .get_anchor_account::<TokenMap>(token_map_address, Default::default())
            .await?;
        let feeds = token_records(&token_map, &tokens)
            .map_err(gmsol_solana_utils::Error::custom)?
            .into_iter()
            .map(SerdeTokenRecord::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)?;

        Ok(Self {
            token_map: (*token_map_address).into(),
            feeds,
            glv_market_tokens,
        })
    }
}

#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Copy)]
pub enum LpTokenKind {
    /// GM.
    Gm,
    /// GLV.
    Glv,
}

struct SharedArgs {
    owner: Pubkey,
    position_id: u64,
    global_state: Pubkey,
    lp_mint: Pubkey,
    position: Pubkey,
    position_vault: Pubkey,
    gt_store: Pubkey,
    gt_program: Pubkey,
}

/// Builder for LP program initialization instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct InitializeLp {
    /// Payer (a.k.a. authority).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Minimum stake value in USD scaled by 1e20.
    pub min_stake_value: u128,
    /// Initial APY for all buckets (1e20-scaled).
    pub initial_apy: u128,
}

impl IntoAtomicGroup for InitializeLp {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;
        let mut insts = AtomicGroup::new(&payer);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::Initialize {
                min_stake_value: self.min_stake_value,
                initial_apy: self.initial_apy,
            })
            .anchor_accounts(
                accounts::Initialize {
                    global_state,
                    authority: payer,
                    system_program: system_program::ID,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token controller creation instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateLpTokenController {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Controller index.
    pub controller_index: u64,
}

impl IntoAtomicGroup for CreateLpTokenController {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();
        let controller = self.lp_program.find_lp_token_controller_address(
            &global_state,
            &self.lp_token_mint.0,
            self.controller_index,
        );

        let instruction = self
            .lp_program
            .anchor_instruction(args::CreateLpTokenController {
                lp_token_mint: self.lp_token_mint.0,
                controller_index: self.controller_index,
            })
            .anchor_accounts(
                accounts::CreateLpTokenController {
                    global_state,
                    controller,
                    authority,
                    system_program: system_program::ID,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token controller disable instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct DisableLpTokenController {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Controller index.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub controller_index: u64,
    /// Controller address (if provided, takes precedence over controller_index).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub controller_address: Option<StringPubkey>,
}

impl IntoAtomicGroup for DisableLpTokenController {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &self.lp_token_mint.0,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        let instruction = self
            .lp_program
            .anchor_instruction(args::DisableLpTokenController {})
            .anchor_accounts(
                accounts::DisableLpTokenController {
                    global_state,
                    controller,
                    gt_store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token unstaking instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UnstakeLpToken {
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token kind.
    pub lp_token_kind: LpTokenKind,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// LP token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub lp_token_account: Option<StringPubkey>,
    /// Position ID.
    pub position_id: u64,
    /// Unstake amount.
    pub unstake_amount: u64,
    /// Controller index.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub controller_index: u64,
    /// Controller address (if provided, takes precedence over controller_index).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub controller_address: Option<StringPubkey>,
}

impl UnstakeLpToken {
    fn lp_token_account(&self, token_program_id: &Pubkey) -> Pubkey {
        self.lp_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                anchor_spl::associated_token::get_associated_token_address_with_program_id(
                    &self.payer,
                    &self.lp_token_mint,
                    token_program_id,
                )
            })
    }

    fn shared_unstake_args(&self) -> SharedUnstakeArgs {
        let owner = self.payer.0;
        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);
        let position_vault = self.lp_program.find_stake_position_vault_address(&position);

        SharedUnstakeArgs {
            owner,
            global_state,
            lp_mint,
            controller,
            position,
            position_vault,
            gt_store: self.store_program.store.0,
            gt_program: *self.store_program.id(),
        }
    }
}

impl IntoAtomicGroup for UnstakeLpToken {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let SharedUnstakeArgs {
            owner,
            global_state,
            lp_mint,
            controller,
            position,
            position_vault,
            gt_store,
            gt_program,
        } = self.shared_unstake_args();

        // Use GT program's find_user_address for gt_user
        let gt_user = crate::pda::find_user_address(&gt_store, &owner, &gt_program).0;
        let event_authority = self.store_program.find_event_authority_address();

        // Token program depends on LP token kind: GM uses token::ID, GLV uses token_2022::ID
        let token_program_id = match self.lp_token_kind {
            LpTokenKind::Gm => anchor_spl::token::ID,
            LpTokenKind::Glv => anchor_spl::token_2022::ID,
        };

        let instruction = self
            .lp_program
            .anchor_instruction(args::UnstakeLp {
                _position_id: self.position_id,
                unstake_amount: self.unstake_amount,
            })
            .anchor_accounts(
                accounts::UnstakeLp {
                    global_state,
                    controller,
                    lp_mint,
                    store: gt_store,
                    gt_program,
                    position,
                    position_vault,
                    owner,
                    gt_user,
                    user_lp_token: self.lp_token_account(&token_program_id),
                    event_authority,
                    token_program: token_program_id,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

struct SharedUnstakeArgs {
    owner: Pubkey,
    global_state: Pubkey,
    lp_mint: Pubkey,
    controller: Pubkey,
    position: Pubkey,
    position_vault: Pubkey,
    gt_store: Pubkey,
    gt_program: Pubkey,
}

/// Builder for transferring LP program authority instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct TransferAuthority {
    /// Current authority.
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// New authority.
    #[builder(setter(into))]
    pub new_authority: StringPubkey,
}

impl IntoAtomicGroup for TransferAuthority {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::TransferAuthority {
                new_authority: self.new_authority.0,
            })
            .anchor_accounts(
                accounts::TransferAuthority {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for accepting LP program authority instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct AcceptAuthority {
    /// Pending authority.
    #[builder(setter(into))]
    pub pending_authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
}

impl IntoAtomicGroup for AcceptAuthority {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let pending_authority = self.pending_authority.0;
        let mut insts = AtomicGroup::new(&pending_authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::AcceptAuthority {})
            .anchor_accounts(
                accounts::AcceptAuthority {
                    global_state,
                    pending_authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for setting pricing staleness configuration instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetPricingStaleness {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Staleness threshold in seconds.
    pub staleness_seconds: u32,
}

impl IntoAtomicGroup for SetPricingStaleness {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::SetPricingStaleness {
                staleness_seconds: self.staleness_seconds,
            })
            .anchor_accounts(
                accounts::SetPricingStaleness {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for setting claim enabled status instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetClaimEnabled {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Whether claiming is enabled.
    pub enabled: bool,
}

impl IntoAtomicGroup for SetClaimEnabled {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::SetClaimEnabled {
                enabled: self.enabled,
            })
            .anchor_accounts(
                accounts::SetClaimEnabled {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating APY gradient with sparse entries instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateApyGradientSparse {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Bucket indices to update.
    pub bucket_indices: Vec<u8>,
    /// APY values (1e20-scaled).
    pub apy_values: Vec<u128>,
}

impl IntoAtomicGroup for UpdateApyGradientSparse {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateApyGradientSparse {
                bucket_indices: self.bucket_indices,
                apy_values: self.apy_values,
            })
            .anchor_accounts(
                accounts::UpdateApyGradientSparse {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating APY gradient with range entries instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateApyGradientRange {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// Start bucket index.
    pub start_bucket: u8,
    /// End bucket index.
    pub end_bucket: u8,
    /// APY values (1e20-scaled).
    pub apy_values: Vec<u128>,
}

impl IntoAtomicGroup for UpdateApyGradientRange {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateApyGradientRange {
                start_bucket: self.start_bucket,
                end_bucket: self.end_bucket,
                apy_values: self.apy_values,
            })
            .anchor_accounts(
                accounts::UpdateApyGradientRange {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for updating minimum stake value instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateMinStakeValue {
    /// Authority (must match GlobalState authority).
    #[builder(setter(into))]
    pub authority: StringPubkey,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// New minimum stake value (1e20-scaled).
    pub new_min_stake_value: u128,
}

impl IntoAtomicGroup for UpdateMinStakeValue {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.authority.0;
        let mut insts = AtomicGroup::new(&authority);

        let global_state = self.lp_program.find_global_state_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::UpdateMinStakeValue {
                new_min_stake_value: self.new_min_stake_value,
            })
            .anchor_accounts(
                accounts::UpdateMinStakeValue {
                    global_state,
                    authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

/// Builder for LP token GT reward calculation instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CalculateGtReward {
    /// Owner of the position.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Position ID.
    pub position_id: u64,
    /// Controller index.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub controller_index: u64,
    /// Controller address (if provided, takes precedence over controller_index).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub controller_address: Option<StringPubkey>,
}

/// Builder for claiming GT rewards instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct ClaimGtReward {
    /// Owner of the position.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Liquidity provider program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub lp_program: LiquidityProviderProgram,
    /// LP token mint address.
    #[builder(setter(into))]
    pub lp_token_mint: StringPubkey,
    /// Position ID.
    pub position_id: u64,
    /// Controller index.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub controller_index: u64,
    /// Controller address (if provided, takes precedence over controller_index).
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub controller_address: Option<StringPubkey>,
}

impl IntoAtomicGroup for CalculateGtReward {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.owner.0;
        let mut insts = AtomicGroup::new(&owner);

        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);

        let instruction = self
            .lp_program
            .anchor_instruction(args::CalculateGtReward {})
            .anchor_accounts(
                accounts::CalculateGtReward {
                    global_state,
                    controller,
                    gt_store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    position,
                    owner,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}

impl IntoAtomicGroup for ClaimGtReward {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.owner.0;
        let mut insts = AtomicGroup::new(&owner);

        let global_state = self.lp_program.find_global_state_address();
        let lp_mint = self.lp_token_mint.0;

        let controller = resolve_controller_address_for_builder(
            &self.lp_program,
            &global_state,
            &lp_mint,
            self.controller_index,
            self.controller_address.as_ref(),
        );

        let position =
            self.lp_program
                .find_stake_position_address(&owner, self.position_id, &controller);

        // Use GT program's find_user_address for gt_user
        let gt_user = crate::pda::find_user_address(
            &self.store_program.store.0,
            &owner,
            self.store_program.id(),
        )
        .0;
        let event_authority = self.store_program.find_event_authority_address();

        let instruction = self
            .lp_program
            .anchor_instruction(args::ClaimGt {
                _position_id: self.position_id,
            })
            .anchor_accounts(
                accounts::ClaimGt {
                    global_state,
                    controller,
                    store: self.store_program.store.0,
                    gt_program: *self.store_program.id(),
                    position,
                    owner,
                    gt_user,
                    event_authority,
                },
                false,
            )
            .build();

        insts.add(instruction);
        Ok(insts)
    }
}
