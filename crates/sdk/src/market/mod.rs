/// Value.
pub mod value;

/// Market Status.
pub mod status;

use gmsol_model::{
    num::{MulDiv, Unsigned},
    num_traits::Zero,
    price::Prices,
    utils::div_to_factor,
    Balance, BalanceExt, BaseMarket, BaseMarketExt, BorrowingFeeMarketExt, PerpMarket,
    PerpMarketExt, PerpMarketMutExt, PnlFactorKind,
};
use gmsol_programs::{constants::MARKET_DECIMALS, model::MarketModel};

use crate::constants;

pub use self::{
    status::MarketStatus,
    value::{SignedValue, Value},
};

/// Market Calculations.
pub trait MarketCalculations {
    /// Calculate market status.
    fn status(&self, prices: &Prices<u128>) -> crate::Result<MarketStatus>;

    /// Calculates max sellable value.
    fn max_sellable_value(&self, prices: &Prices<u128>) -> crate::Result<u128>;
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
                let size_of_paying_side = if longs_pay_shorts {
                    open_interest_for_long
                } else {
                    open_interest_for_short
                };
                let funding_rate_per_second_for_long = if longs_pay_shorts {
                    funding_factor_per_second
                        .checked_mul_div_ceil(&size_of_paying_side, &open_interest_for_long)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for long")
                        })?
                        .to_signed()?
                } else {
                    funding_factor_per_second
                        .checked_mul_div(&size_of_paying_side, &open_interest_for_long)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for long")
                        })?
                        .to_opposite_signed()?
                };
                let funding_rate_per_second_for_short = if !longs_pay_shorts {
                    funding_factor_per_second
                        .checked_mul_div_ceil(&size_of_paying_side, &open_interest_for_short)
                        .ok_or_else(|| {
                            crate::Error::custom("failed to calculate funding rate for short")
                        })?
                        .to_signed()?
                } else {
                    funding_factor_per_second
                        .checked_mul_div(&size_of_paying_side, &open_interest_for_short)
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

    fn max_sellable_value(&self, prices: &Prices<u128>) -> crate::Result<u128> {
        fn max_sellable_value_for_one_side(
            market: &MarketModel,
            prices: &Prices<u128>,
            is_long: bool,
        ) -> crate::Result<u128> {
            let index_token_price = &prices.index_token_price;

            // Calculate min pool value according to the reserve factor.
            let reserved_value = market.reserved_value(index_token_price, is_long)?;
            let reserve_factor = market.reserve_factor()?;
            let mut min_pool_value =
                div_to_factor::<_, { MARKET_DECIMALS }>(&reserved_value, &reserve_factor, true)
                    .ok_or_else(|| {
                        crate::Error::custom(
                            "failed to calculate min pool value according to reserve factor",
                        )
                    })?;

            // Calculate min pool value according to the pnl factor.
            let pnl = market.pnl(&prices.index_token_price, is_long, true)?;
            if pnl.is_positive() {
                let pnl_factor =
                    market.pnl_factor_config(PnlFactorKind::MaxAfterWithdrawal, is_long)?;
                let min_pool_value_for_pnl_factor =
                    div_to_factor::<_, { MARKET_DECIMALS }>(&pnl.unsigned_abs(), &pnl_factor, true)
                        .ok_or_else(|| {
                            crate::Error::custom(
                                "failed to calculate min pool value according to pnl factor",
                            )
                        })?;
                min_pool_value = min_pool_value.max(min_pool_value_for_pnl_factor);
            }

            let pool_value = market.pool_value_without_pnl_for_one_side(prices, is_long, false)?;

            pool_value.checked_sub(min_pool_value).ok_or_else(|| {
                crate::Error::custom(format!(
                    "failed to calculate max sellable value for one side, is_long={is_long}"
                ))
            })
        }

        let max_sellable_value_for_long = max_sellable_value_for_one_side(self, prices, true)?;
        let max_sellable_value_for_short = max_sellable_value_for_one_side(self, prices, false)?;

        let liquidity_pool = self.liquidity_pool()?;
        let liquidity_pool_value_for_long =
            liquidity_pool.long_usd_value(prices.long_token_price.pick_price(true))?;
        let liquidity_pool_value_for_short =
            liquidity_pool.short_usd_value(prices.short_token_price.pick_price(true))?;
        let liquidity_pool_value = liquidity_pool_value_for_long
            .checked_add(liquidity_pool_value_for_short)
            .ok_or_else(|| crate::Error::custom("liquidity pool value overflow"))?;

        max_sellable_value_for_long
            .checked_mul_div(&liquidity_pool_value, &liquidity_pool_value_for_long)
            .and_then(|scaled_for_long| {
                let scaled_for_short = max_sellable_value_for_short
                    .checked_mul_div(&liquidity_pool_value, &liquidity_pool_value_for_short)?;
                Some(scaled_for_long.min(scaled_for_short))
            })
            .ok_or_else(|| crate::Error::custom("failed to calculate max sellable value"))
    }
}
