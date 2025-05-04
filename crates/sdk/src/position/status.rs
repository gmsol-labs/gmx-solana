/// Position Status.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct PositionStatus {
    /// Entry price.
    pub entry_price: u128,
    /// Collateral value.
    pub collateral_value: u128,
    /// Pending PnL.
    pub pending_pnl: i128,
    /// Pending borrowing fee value.
    pub pending_borrowing_fee_value: u128,
    /// Pending funding fee value.
    pub pending_funding_fee_value: u128,
    /// Pending claimable funding fee value in long token.
    pub pending_claimable_funding_fee_value_in_long_token: u128,
    /// Pending claimable funding fee value in short token.
    pub pending_claimable_funding_fee_value_in_short_token: u128,
    /// Close order fee value.
    pub close_order_fee_value: u128,
    /// Net value.
    pub net_value: i128,
    /// Leverage.
    pub leverage: Option<u128>,
    /// Liquidation price.
    pub liquidation_price: Option<i128>,
}
