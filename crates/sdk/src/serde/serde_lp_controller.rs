#[cfg(liquidity_provider)]
use gmsol_programs::gmsol_liquidity_provider::accounts::LpTokenController;

use super::StringPubkey;

/// Serializable version of LP token controller [`LpTokenController`].
#[cfg(liquidity_provider)]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeLpController {
    /// Controller index (allows multiple controllers per token).
    pub controller_index: u64,
    /// Controller address (PDA derived from global_state + lp_token_mint + controller_index).
    pub controller_address: StringPubkey,
    /// Associated global_state.
    pub global_state: StringPubkey,
    /// Corresponding LP token mint.
    pub lp_token_mint: StringPubkey,
    /// Current number of active positions.
    pub total_positions: u64,
    /// Whether staking is enabled (default true, irreversible when set to false).
    pub is_enabled: bool,
    /// Timestamp when disabled (only valid when is_enabled = false, display as formatted date).
    pub disabled_at: i64,
    /// Cumulative inverse cost factor snapshot when disabled (only valid when is_enabled = false).
    pub disabled_cum_inv_cost: String,
}

#[cfg(liquidity_provider)]
impl SerdeLpController {
    /// Create from LP [`LpTokenController`] with controller address.
    pub fn from_controller(
        controller: &LpTokenController,
        controller_address: &solana_sdk::pubkey::Pubkey,
    ) -> Self {
        Self {
            controller_index: controller.controller_index,
            controller_address: (*controller_address).into(),
            global_state: controller.global_state.into(),
            lp_token_mint: controller.lp_token_mint.into(),
            total_positions: controller.total_positions,
            is_enabled: controller.is_enabled,
            disabled_at: controller.disabled_at,
            disabled_cum_inv_cost: controller.disabled_cum_inv_cost.to_string(),
        }
    }
}
