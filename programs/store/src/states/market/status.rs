use anchor_lang::prelude::*;
use gmsol_model::{price::Prices, BaseMarketExt, BorrowingFeeMarketExt, PerpMarket};

use super::Market;

/// Market Status.
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct MarketStatus {
    /// Funding factor per second.
    pub funding_factor_per_second: i128,
    /// Borrowing factor per second for long.
    pub borrowing_factor_per_second_for_long: u128,
    /// Borrowing factor per second for short.
    pub borrowing_factor_per_second_for_short: u128,
    /// Pending pnl for long.
    pub pending_pnl_for_long: i128,
    /// Pending pnl for short.
    pub pending_pnl_for_short: i128,
    /// Reserve value for long.
    pub reserve_value_for_long: u128,
    /// Reserve value for short.
    pub reserve_value_for_short: u128,
    /// Pool value without pnl for long.
    pub pool_value_without_pnl_for_long: u128,
    /// Pool value without pnl for short.
    pub pool_value_without_pnl_for_short: u128,
}

impl MarketStatus {
    /// Create from market and prices.
    pub fn from_market(
        market: &Market,
        prices: &Prices<u128>,
        maximize_pnl: bool,
        maximize_pool_value: bool,
    ) -> gmsol_model::Result<Self> {
        Ok(Self {
            funding_factor_per_second: *market.funding_factor_per_second(),
            borrowing_factor_per_second_for_long: market
                .borrowing_factor_per_second(true, prices)?,
            borrowing_factor_per_second_for_short: market
                .borrowing_factor_per_second(false, prices)?,
            pending_pnl_for_long: market.pnl(&prices.index_token_price, true, maximize_pnl)?,
            pending_pnl_for_short: market.pnl(&prices.index_token_price, false, maximize_pnl)?,
            reserve_value_for_long: market.reserved_value(&prices.index_token_price, true)?,
            reserve_value_for_short: market.reserved_value(&prices.index_token_price, false)?,
            pool_value_without_pnl_for_long: market.pool_value_without_pnl_for_one_side(
                prices,
                true,
                maximize_pool_value,
            )?,
            pool_value_without_pnl_for_short: market.pool_value_without_pnl_for_one_side(
                prices,
                false,
                maximize_pool_value,
            )?,
        })
    }
}
