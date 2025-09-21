#[cfg(liquidity_provider)]
use gmsol_programs::gmsol_liquidity_provider::accounts::{LpTokenController, Position};

use crate::utils::{Amount, GmAmount, Value};

use super::StringPubkey;

/// Additional computed data for LP position.
#[cfg(liquidity_provider)]
#[derive(Debug, Clone)]
pub struct LpPositionComputedData {
    /// Claimable GT rewards (calculated using precise on-chain logic) - u128 to avoid overflow.
    pub claimable_gt: u128,
    /// Current display APY (current week's APY rate) as fixed-point Value (1e20 scale, same as on-chain).
    /// Note: This is used for UI display and represents the APY rate for the current staking week.
    /// GT reward calculations internally use time-weighted APY for accuracy.
    pub current_apy: Value,
    /// LP token symbol (e.g., "GM-SOL/USDC", "GLV-BTC").
    /// Should have fallback to abbreviated mint address if mapping fails.
    pub lp_token_symbol: String,
}

/// Serializable version of LP staking position [`Position`].
#[cfg(liquidity_provider)]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeLpStakingPosition {
    /// Owner of this LP position.
    pub owner: StringPubkey,
    /// LP token controller that manages this position.
    pub controller: StringPubkey,
    /// LP token mint for this position.
    pub lp_token_mint: StringPubkey,
    /// LP token symbol (e.g., "GM-SOL/USDC", "GLV-BTC").
    pub lp_token_symbol: String,
    /// Position id to allow multiple positions per owner.
    pub position_id: u64,
    /// Staked LP amount (raw format, display layer will format).
    pub staked_amount: GmAmount,
    /// Staked value in USD (raw format, display layer will format).
    pub staked_value_usd: Value,
    /// Stake start unix timestamp (seconds) - display layer handles formatting.
    pub stake_start_time: i64,
    /// Current display APY (current week's APY rate) as fixed-point Value (display layer converts to percentage).
    /// Note: This is the APY for the current staking week, used for UI display.
    /// For GT reward calculations, time-weighted APY is used internally.
    pub current_apy: Value,
    /// Claimable GT rewards (calculated using precise on-chain logic) - raw format.
    pub claimable_gt: Amount,
    /// Position vault address (PDA that holds staked tokens).
    pub vault: StringPubkey,
    /// Whether the controller is still enabled.
    pub controller_enabled: bool,
}

#[cfg(liquidity_provider)]
impl SerdeLpStakingPosition {
    /// Create from LP [`Position`] with additional computed data.
    pub fn from_position(
        position: &Position,
        controller: &LpTokenController,
        computed_data: LpPositionComputedData,
        gt_decimals: u8,
    ) -> crate::Result<Self> {
        // Use Amount::from_u128 to avoid silent truncation - will return error if overflow
        let claimable_gt =
            Amount::from_u128(computed_data.claimable_gt, gt_decimals).map_err(|_| {
                crate::Error::custom("Claimable GT amount exceeds maximum representable value")
            })?;

        // Symbol fallback: use provided symbol or generate from mint address
        let lp_token_symbol = if computed_data.lp_token_symbol.is_empty() {
            fallback_lp_token_symbol(&position.lp_mint.into())
        } else {
            computed_data.lp_token_symbol
        };

        Ok(Self {
            owner: position.owner.into(),
            controller: position.controller.into(),
            lp_token_mint: position.lp_mint.into(),
            lp_token_symbol,
            position_id: position.position_id,
            staked_amount: GmAmount::from_u64(position.staked_amount),
            staked_value_usd: Value::from_u128(position.staked_value_usd),
            stake_start_time: position.stake_start_time, // Raw timestamp, display layer formats
            current_apy: computed_data.current_apy,      // Raw Value, display layer converts to %
            claimable_gt,
            vault: position.vault.into(),
            controller_enabled: controller.is_enabled,
        })
    }
}

/// Helper to create a fallback symbol from mint address when token mapping fails.
#[cfg(liquidity_provider)]
pub fn fallback_lp_token_symbol(mint: &StringPubkey) -> String {
    let mint_str = mint.to_string();
    // Take first 4 and last 4 characters for abbreviated display
    if mint_str.len() > 8 {
        format!("{}...{}", &mint_str[..4], &mint_str[mint_str.len() - 4..])
    } else {
        mint_str
    }
}
