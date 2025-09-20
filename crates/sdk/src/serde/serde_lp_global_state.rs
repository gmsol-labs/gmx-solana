use gmsol_programs::gmsol_liquidity_provider::accounts::GlobalState;

use crate::utils::Value;

use super::StringPubkey;

/// Serializable version of LP Global State [`GlobalState`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeLpGlobalState {
    /// Program administrator with governance privileges.
    pub authority: StringPubkey,
    /// Pending authority awaiting acceptance (default if none).
    pub pending_authority: StringPubkey,
    /// APY gradient buckets (53 buckets), each is 1e20-scaled APY for weekly buckets.
    /// Buckets represent: [0-1), [1-2), ..., [52-+inf) weeks.
    pub apy_gradient: Vec<Value>,
    /// Minimum stake value in USD (1e20-scaled).
    pub min_stake_value: Value,
    /// If true, LPs may call `claim_gt` at any time without unstaking.
    pub claim_enabled: bool,
    /// Price staleness configuration in seconds.
    pub pricing_staleness_seconds: u32,
    /// PDA bump for this GlobalState.
    pub bump: u8,
}

impl SerdeLpGlobalState {
    /// Create from LP [`GlobalState`].
    pub fn from_global_state(global_state: &GlobalState) -> Self {
        // Convert APY gradient array to Vec of Values for serialization
        let apy_gradient = global_state
            .apy_gradient
            .iter()
            .map(|&apy| Value::from_u128(apy))
            .collect();

        Self {
            authority: global_state.authority.into(),
            pending_authority: global_state.pending_authority.into(),
            apy_gradient,
            min_stake_value: Value::from_u128(global_state.min_stake_value),
            claim_enabled: global_state.claim_enabled,
            pricing_staleness_seconds: global_state.pricing_staleness_seconds,
            bump: global_state.bump,
        }
    }
}
