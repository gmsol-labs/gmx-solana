/// Value.
pub mod value;

/// Market Status.
pub mod status;

use gmsol_model::{
    num::{MulDiv, Unsigned},
    num_traits::Zero,
    price::Prices,
    Balance, BaseMarket, BaseMarketExt, BorrowingFeeMarketExt, PerpMarket, PerpMarketExt,
    PerpMarketMutExt,
};
use gmsol_programs::model::MarketModel;

use crate::constants;

pub use self::{
    status::MarketStatus,
    value::{SignedValue, Value},
};

/// Market Calculations.
pub trait MarketCalculations {
    /// Calculate market status.
    fn status(&self, prices: &Prices<u128>) -> crate::Result<MarketStatus>;
}

impl MarketCalculations for MarketModel {
    fn status(&self, prices: &Prices<u128>) -> crate::Result<MarketStatus> {
        // Calculate open interests.
        let open_interest = self.open_interest()?;
        let open_interest_for_long = open_interest.long_amount()?;
        let open_interest_for_short = open_interest.short_amount()?;
        let open_interest_in_tokens = self.open_interest_in_tokens()?;

        // Calculate funding rates.
        let (funding_rate_per_second_for_long, funding_rate_per_second_for_short) = {
            if open_interest_for_long == 0 || open_interest_for_short == 0 {
                (0, 0)
            } else {
                let (funding_factor_per_second, longs_pay_shorts, _) = self
                    .clone()
                    .update_funding(prices)?
                    .next_funding_factor_per_second(
                        self.passed_in_seconds_for_funding()?,
                        &open_interest_for_long,
                        &open_interest_for_short,
                    )?;
                let size_for_larger_side = open_interest_for_long.max(open_interest_for_short);
                let funding_rate_per_second_for_long = if longs_pay_shorts {
                    funding_factor_per_second
                        .checked_mul_div_ceil(&size_for_larger_side, &open_interest_for_long)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for long")
                        })?
                        .to_signed()?
                } else {
                    funding_factor_per_second
                        .checked_mul_div(&size_for_larger_side, &open_interest_for_long)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for long")
                        })?
                        .to_opposite_signed()?
                };
                let funding_rate_per_second_for_short = if !longs_pay_shorts {
                    funding_factor_per_second
                        .checked_mul_div_ceil(&size_for_larger_side, &open_interest_for_short)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for short")
                        })?
                        .to_signed()?
                } else {
                    funding_factor_per_second
                        .checked_mul_div(&size_for_larger_side, &open_interest_for_short)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for short")
                        })?
                        .to_opposite_signed()?
                };

                (
                    funding_rate_per_second_for_long,
                    funding_rate_per_second_for_short,
                )
            }
        };

        // Calculate liquidities.
        let reserved_value_for_long = self.reserved_value(&prices.index_token_price, true)?;
        let reserved_value_for_short = self.reserved_value(&prices.index_token_price, false)?;
        let pool_value_without_pnl_for_long = Value {
            min: self.pool_value_without_pnl_for_one_side(prices, true, false)?,
            max: self.pool_value_without_pnl_for_one_side(prices, true, true)?,
        };
        let pool_value_without_pnl_for_short = Value {
            min: self.pool_value_without_pnl_for_one_side(prices, false, false)?,
            max: self.pool_value_without_pnl_for_one_side(prices, false, true)?,
        };
        let reserve_factor = self
            .reserve_factor()?
            .min(self.open_interest_reserve_factor()?);
        let max_reserve_value_for_long = gmsol_model::utils::apply_factor::<
            _,
            { constants::MARKET_DECIMALS },
        >(
            &pool_value_without_pnl_for_long.min, &reserve_factor
        )
        .ok_or_else(|| crate::Error::custom("failed to calculate max reserved value for long"))?;
        let max_reserve_value_for_short = gmsol_model::utils::apply_factor::<
            _,
            { constants::MARKET_DECIMALS },
        >(
            &pool_value_without_pnl_for_short.min, &reserve_factor
        )
        .ok_or_else(|| crate::Error::custom("failed to calculate max reserved value for short"))?;
        let max_liquidity_for_long = max_reserve_value_for_long.min(self.max_open_interest(true)?);
        let max_liquidity_for_short =
            max_reserve_value_for_short.min(self.max_open_interest(false)?);

        // Calculate min collateral factor.
        let min_collateral_factor = *self.position_params()?.min_collateral_factor();
        let min_collateral_factor_for_long = self
            .min_collateral_factor_for_open_interest(&Zero::zero(), true)?
            .max(min_collateral_factor);
        let min_collateral_factor_for_short = self
            .min_collateral_factor_for_open_interest(&Zero::zero(), false)?
            .max(min_collateral_factor);

        Ok(MarketStatus {
            funding_rate_per_second_for_long,
            funding_rate_per_second_for_short,
            borrowing_rate_per_second_for_long: self.borrowing_factor_per_second(true, prices)?,
            borrowing_rate_per_second_for_short: self.borrowing_factor_per_second(false, prices)?,
            pending_pnl_for_long: SignedValue {
                min: self.pnl(&prices.index_token_price, true, false)?,
                max: self.pnl(&prices.index_token_price, true, true)?,
            },
            pending_pnl_for_short: SignedValue {
                min: self.pnl(&prices.index_token_price, false, false)?,
                max: self.pnl(&prices.index_token_price, false, true)?,
            },
            reserved_value_for_long,
            reserved_value_for_short,
            max_reserve_value_for_long,
            max_reserve_value_for_short,
            pool_value_without_pnl_for_long,
            pool_value_without_pnl_for_short,
            liquidity_for_long: max_liquidity_for_long.saturating_sub(reserved_value_for_long),
            liquidity_for_short: max_liquidity_for_short.saturating_sub(reserved_value_for_short),
            max_liquidity_for_long,
            max_liquidity_for_short,
            open_interest_for_long,
            open_interest_for_short,
            open_interest_in_tokens_for_long: open_interest_in_tokens.long_amount()?,
            open_interest_in_tokens_for_short: open_interest_in_tokens.short_amount()?,
            min_collateral_factor_for_long,
            min_collateral_factor_for_short,
        })
    }
}
