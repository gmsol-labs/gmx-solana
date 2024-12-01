use num_traits::Zero;

use crate::{
    fixed::FixedPointOps,
    market::{BaseMarket, BaseMarketExt, PerpMarketMutExt},
    num::{MulDiv, Unsigned},
    params::fee::FundingRateChangeType,
    price::Prices,
    Balance, BalanceExt, PerpMarketMut,
};

/// Update Funding State Action.
#[must_use]
pub struct UpdateFundingState<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
    prices: Prices<M::Num>,
}

impl<M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> UpdateFundingState<M, DECIMALS> {
    /// Create a new [`UpdateFundingState`] action.
    pub fn try_new(market: M, prices: &Prices<M::Num>) -> crate::Result<Self> {
        prices.validate()?;
        Ok(Self {
            market,
            prices: prices.clone(),
        })
    }

    /// Execute.
    pub fn execute(mut self) -> crate::Result<UpdateFundingReport<M::Num>> {
        const MATRIX: [(bool, bool); 4] =
            [(true, true), (true, false), (false, true), (false, false)];
        let duration_in_seconds = self.market.just_passed_in_seconds_for_funding()?;
        let report = self.next_funding_amount_per_size(duration_in_seconds)?;
        for (is_long, is_long_collateral) in MATRIX {
            self.market.apply_delta_to_funding_amount_per_size(
                is_long,
                is_long_collateral,
                &report
                    .delta_funding_amount_per_size(is_long, is_long_collateral)
                    .to_signed()?,
            )?;
            self.market
                .apply_delta_to_claimable_funding_amount_per_size(
                    is_long,
                    is_long_collateral,
                    &report
                        .delta_claimable_funding_amount_per_size(is_long, is_long_collateral)
                        .to_signed()?,
                )?;
        }
        *self.market.funding_factor_per_second_mut() =
            report.next_funding_factor_per_second().clone();
        Ok(report)
    }

    fn next_funding_amount_per_size(
        &self,
        duration_in_seconds: u64,
    ) -> crate::Result<UpdateFundingReport<M::Num>> {
        use crate::utils;
        use num_traits::{CheckedMul, FromPrimitive};

        let mut report = UpdateFundingReport::empty(duration_in_seconds);
        let open_interest = self.market.open_interest()?;
        let long_open_interest = open_interest.long_amount()?;
        let short_open_interest = open_interest.short_amount()?;

        if long_open_interest.is_zero() || short_open_interest.is_zero() {
            return Ok(report);
        }

        let (funding_factor_per_second, longs_pay_shorts, next_funding_factor_per_second) = self
            .next_funding_factor_per_second(
                duration_in_seconds,
                &long_open_interest,
                &short_open_interest,
            )?;
        report.next_funding_factor_per_second = next_funding_factor_per_second;

        let size_of_larger_side = if long_open_interest > short_open_interest {
            long_open_interest.clone()
        } else {
            short_open_interest.clone()
        };
        let duration_value = M::Num::from_u64(duration_in_seconds).ok_or(crate::Error::Convert)?;
        let funding_factor = duration_value
            .checked_mul(&funding_factor_per_second)
            .ok_or(crate::Error::Computation("calculating funding factor"))?;
        let funding_value = utils::apply_factor(&size_of_larger_side, &funding_factor)
            .ok_or(crate::Error::Computation("calculating funding value"))?;

        let payer_open_interest = if longs_pay_shorts {
            &long_open_interest
        } else {
            &short_open_interest
        };
        let for_long_collateral = funding_value
            .checked_mul_div(
                &self
                    .market
                    .open_interest_pool(longs_pay_shorts)?
                    .long_amount()?,
                payer_open_interest,
            )
            .ok_or(crate::Error::Computation(
                "calculating funding value for long collateral",
            ))?;
        let for_short_collateral = funding_value
            .checked_mul_div(
                &self
                    .market
                    .open_interest_pool(longs_pay_shorts)?
                    .short_amount()?,
                payer_open_interest,
            )
            .ok_or(crate::Error::Computation(
                "calculating funding value for short collateral",
            ))?;

        self.set_deltas(
            &mut report,
            longs_pay_shorts,
            &for_long_collateral,
            &for_short_collateral,
            if !longs_pay_shorts {
                &long_open_interest
            } else {
                &short_open_interest
            },
        )?;

        Ok(report)
    }

    fn set_deltas(
        &self,
        report: &mut UpdateFundingReport<M::Num>,
        longs_pay_shorts: bool,
        for_long_collateral: &M::Num,
        for_short_collateral: &M::Num,
        receiver_interest: &M::Num,
    ) -> crate::Result<()> {
        let adjustment = &self.market.funding_amount_per_size_adjustment();
        for is_long_collateral in [true, false] {
            let (funding_value, price) = if is_long_collateral {
                (
                    for_long_collateral,
                    self.prices.long_token_price.pick_price(true),
                )
            } else {
                (
                    for_short_collateral,
                    self.prices.short_token_price.pick_price(true),
                )
            };

            let payer = flags_to_index(longs_pay_shorts, is_long_collateral);
            let receiver = flags_to_index(!longs_pay_shorts, is_long_collateral);

            report.delta_funding_amount_per_size[payer] = pack_to_funding_amount_per_size(
                adjustment,
                funding_value,
                &self
                    .market
                    .open_interest_pool(longs_pay_shorts)?
                    .amount(is_long_collateral)?,
                price,
                true,
            )
            .ok_or(crate::Error::Computation(
                "calculating delta funding amount per size",
            ))?;

            report.delta_claimable_funding_amount_per_size[receiver] =
                pack_to_funding_amount_per_size(
                    adjustment,
                    funding_value,
                    receiver_interest,
                    price,
                    false,
                )
                .ok_or(crate::Error::Computation(
                    "calculating delta claimable funding amount per size",
                ))?;
        }
        Ok(())
    }

    fn next_funding_factor_per_second(
        &self,
        duration_in_seconds: u64,
        long_open_interest: &M::Num,
        short_open_interest: &M::Num,
    ) -> crate::Result<(M::Num, bool, M::Signed)> {
        use crate::{num::UnsignedAbs, utils};
        use num_traits::{CheckedAdd, CheckedMul, CheckedSub, FromPrimitive, Signed};

        let diff_value = long_open_interest.clone().diff(short_open_interest.clone());

        if diff_value.is_zero() {
            return Ok((Zero::zero(), true, Zero::zero()));
        }

        let total_open_interest = long_open_interest
            .checked_add(short_open_interest)
            .ok_or(crate::Error::Computation("calculating total open interest"))?;

        // `total_open_interest` must be non-zero here, since if `total_open_interest` were zero, the `diff_value` must be zero too.
        debug_assert!(
            !total_open_interest.is_zero(),
            "this should not be possible"
        );

        let params = self.market.funding_fee_params()?;

        let diff_value_after_exponent =
            utils::apply_exponent_factor(diff_value, params.exponent().clone()).ok_or(
                crate::Error::Computation("applying exponent factor to diff value"),
            )?;
        let diff_value_to_open_interest_factor =
            utils::div_to_factor(&diff_value_after_exponent, &total_open_interest, false).ok_or(
                crate::Error::Computation("calculating diff value to open interest factor"),
            )?;

        if params.increase_factor_per_second().is_zero() {
            let mut funding_factor_per_second =
                utils::apply_factor(&diff_value_to_open_interest_factor, params.factor()).ok_or(
                    crate::Error::Computation("calculating fallback funding factor per second"),
                )?;

            if funding_factor_per_second > *params.max_factor_per_second() {
                funding_factor_per_second = params.max_factor_per_second().clone();
            }

            return Ok((
                funding_factor_per_second,
                long_open_interest > short_open_interest,
                Zero::zero(),
            ));
        }

        let funding_factor_per_second = self.market.funding_factor_per_second();
        let funding_factor_per_second_magnitude = funding_factor_per_second.unsigned_abs();

        let change = params.change(
            funding_factor_per_second,
            long_open_interest,
            short_open_interest,
            &diff_value_to_open_interest_factor,
        );

        let duration_value = M::Num::from_u64(duration_in_seconds).ok_or(crate::Error::Convert)?;
        let next_funding_factor_per_second = match change {
            FundingRateChangeType::Increase => {
                let increase_value = utils::apply_factor(
                    &diff_value_to_open_interest_factor,
                    params.increase_factor_per_second(),
                )
                .and_then(|v| v.checked_mul(&duration_value))
                .ok_or(crate::Error::Computation(
                    "calculating factor increase value",
                ))?;

                let increase_value = if long_open_interest < short_open_interest {
                    increase_value.to_opposite_signed()?
                } else {
                    increase_value.to_signed()?
                };

                funding_factor_per_second
                    .checked_add(&increase_value)
                    .ok_or(crate::Error::Computation("increasing funding factor"))?
            }
            FundingRateChangeType::Decrease if !funding_factor_per_second_magnitude.is_zero() => {
                let decrease_value = params
                    .decrease_factor_per_second()
                    .checked_mul(&duration_value)
                    .ok_or(crate::Error::Computation(
                        "calculating factor decrease value",
                    ))?;
                if funding_factor_per_second_magnitude <= decrease_value {
                    funding_factor_per_second.clone()
                        / funding_factor_per_second_magnitude.to_signed()?
                } else {
                    let decreased = funding_factor_per_second_magnitude
                        .checked_sub(&decrease_value)
                        .ok_or(crate::Error::Computation(
                            "calculating decreased funding factor per second (infallible)",
                        ))?;
                    if funding_factor_per_second.is_negative() {
                        decreased.to_opposite_signed()?
                    } else {
                        decreased.to_signed()?
                    }
                }
            }
            _ => funding_factor_per_second.clone(),
        };

        let next_funding_factor_per_second = Unsigned::bound_magnitude(
            &next_funding_factor_per_second,
            &Zero::zero(),
            params.max_factor_per_second(),
        )?;

        let next_funding_factor_per_second_with_min_bound = Unsigned::bound_magnitude(
            &next_funding_factor_per_second,
            params.min_factor_per_second(),
            params.max_factor_per_second(),
        )?;

        Ok((
            next_funding_factor_per_second_with_min_bound.unsigned_abs(),
            next_funding_factor_per_second_with_min_bound.is_positive(),
            next_funding_factor_per_second,
        ))
    }
}

/// Update Funding Report.
#[derive(Debug)]
pub struct UpdateFundingReport<T: Unsigned> {
    duration_in_seconds: u64,
    next_funding_factor_per_second: T::Signed,
    delta_funding_amount_per_size: [T; 4],
    delta_claimable_funding_amount_per_size: [T; 4],
}

#[inline]
fn flags_to_index(is_long: bool, is_long_collateral: bool) -> usize {
    match (is_long_collateral, is_long) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

impl<T: Unsigned> UpdateFundingReport<T> {
    /// Create a new empty report.
    pub fn empty(duration_in_seconds: u64) -> Self {
        Self {
            duration_in_seconds,
            next_funding_factor_per_second: Zero::zero(),
            delta_funding_amount_per_size: [Zero::zero(), Zero::zero(), Zero::zero(), Zero::zero()],
            delta_claimable_funding_amount_per_size: [
                Zero::zero(),
                Zero::zero(),
                Zero::zero(),
                Zero::zero(),
            ],
        }
    }

    /// Get considered duration in seconds.
    pub fn duration_in_seconds(&self) -> u64 {
        self.duration_in_seconds
    }

    /// Get next funding factor per second.
    #[inline]
    pub fn next_funding_factor_per_second(&self) -> &T::Signed {
        &self.next_funding_factor_per_second
    }

    /// Get delta to funding amount per size.
    #[inline]
    pub fn delta_funding_amount_per_size(&self, is_long: bool, is_long_collateral: bool) -> &T {
        let idx = flags_to_index(is_long, is_long_collateral);
        &self.delta_funding_amount_per_size[idx]
    }

    /// Get delta to claimable funding amount per size.
    #[inline]
    pub fn delta_claimable_funding_amount_per_size(
        &self,
        is_long: bool,
        is_long_collateral: bool,
    ) -> &T {
        let idx = flags_to_index(is_long, is_long_collateral);
        &self.delta_claimable_funding_amount_per_size[idx]
    }
}

/// Pack the value to funding amount per size with the given `adjustment`.
pub fn pack_to_funding_amount_per_size<T, const DECIMALS: u8>(
    adjustment: &T,
    funding_value: &T,
    open_interest: &T,
    price: &T,
    round_up_magnitude: bool,
) -> Option<T>
where
    T: FixedPointOps<DECIMALS>,
{
    if funding_value.is_zero() || open_interest.is_zero() {
        return Some(Zero::zero());
    }

    let numerator = adjustment.checked_mul(&T::UNIT)?;
    let funding_value_per_size = if round_up_magnitude {
        funding_value.checked_mul_div_ceil(&numerator, open_interest)?
    } else {
        funding_value.checked_mul_div(&numerator, open_interest)?
    };

    debug_assert!(!price.is_zero(), "must be non-zero");
    if round_up_magnitude {
        funding_value_per_size.checked_round_up_div(price)
    } else {
        Some(funding_value_per_size / price.clone())
    }
}

/// Calculate the funding amount for a position and unpack with the given `adjustment`.
pub fn unpack_to_funding_amount_delta<T, const DECIMALS: u8>(
    adjustment: &T,
    latest_funding_amount_per_size: &T,
    position_funding_amount_per_size: &T,
    size_in_usd: &T,
    round_up_magnitude: bool,
) -> Option<T>
where
    T: FixedPointOps<DECIMALS>,
{
    let funding_diff_factor =
        latest_funding_amount_per_size.checked_sub(position_funding_amount_per_size)?;

    let adjustment = adjustment.checked_mul(&T::UNIT)?;
    if round_up_magnitude {
        size_in_usd.checked_mul_div_ceil(&funding_diff_factor, &adjustment)
    } else {
        size_in_usd.checked_mul_div(&funding_diff_factor, &adjustment)
    }
}

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use crate::{
        market::LiquidityMarketMutExt,
        test::{TestMarket, TestPosition},
        PositionMutExt,
    };

    use super::*;

    #[test]
    fn test_update_funding_state() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market
            .deposit(1_000_000_000_000, 100_000_000_000_000, prices)?
            .execute()?;
        println!("{market:#?}");
        let mut long = TestPosition::long(true);
        let mut short = TestPosition::short(false);
        let prices = Prices::new_for_test(123, 123, 1);
        let report = long
            .ops(&mut market)
            .increase(prices, 1_000_000_000_000, 50_000_000_000_000, None)?
            .execute()?;
        println!("{report:#?}");
        let report = short
            .ops(&mut market)
            .increase(prices, 100_000_000_000_000, 25_000_000_000_000, None)?
            .execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        sleep(Duration::from_secs(2));
        let report = long
            .ops(&mut market)
            .decrease(prices, 50_000_000_000_000, None, 0, Default::default())?
            .execute()?;
        println!("{report:#?}");
        let report = short
            .ops(&mut market)
            .decrease(prices, 25_000_000_000_000, None, 0, Default::default())?
            .execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        Ok(())
    }
}
