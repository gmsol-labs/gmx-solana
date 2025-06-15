use super::{SignedValue, Value};

/// Market Status.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct MarketStatus {
    /// Funding fee rate per hour for long.
    pub funding_rate_per_second_for_long: i128,
    /// Funding fee rate per hour for short.
    pub funding_rate_per_second_for_short: i128,
    /// Borrowing fee rate per second for long.
    pub borrowing_rate_per_second_for_long: u128,
    /// Borrowing fee rate per second for short.
    pub borrowing_rate_per_second_for_short: u128,
    /// Pending pnl for long.
    pub pending_pnl_for_long: SignedValue,
    /// Pending pnl for short.
    pub pending_pnl_for_short: SignedValue,
    /// Reserved value for long.
    pub reserved_value_for_long: u128,
    /// Reserved value for short.
    pub reserved_value_for_short: u128,
    /// Max reserve value for long.
    pub max_reserve_value_for_long: u128,
    /// Max reserve value for short.
    pub max_reserve_value_for_short: u128,
    /// Pool value without pnl for long.
    pub pool_value_without_pnl_for_long: Value,
    /// Pool value without pnl for short.
    pub pool_value_without_pnl_for_short: Value,
    /// Liquidity for long.
    pub liquidity_for_long: u128,
    /// Liquidity for short.
    pub liquidity_for_short: u128,
    /// Max liquidity for long.
    pub max_liquidity_for_long: u128,
    /// Max liquidity for short.
    pub max_liquidity_for_short: u128,
    /// Open interest for long.
    pub open_interest_for_long: u128,
    /// Open interest for short.
    pub open_interest_for_short: u128,
    /// Open interest in tokens for long.
    pub open_interest_in_tokens_for_long: u128,
    /// Open interest in tokens for short.
    pub open_interest_in_tokens_for_short: u128,
    /// Min collateral factor for long.
    pub min_collateral_factor_for_long: u128,
    /// Min collateral factor for short.
    pub min_collateral_factor_for_short: u128,
}
