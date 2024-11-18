use num_traits::{CheckedAdd, CheckedSub, Zero};

use crate::{
    params::fee::BorrowingFeeParams, price::Prices, Balance, BalanceExt, BaseMarket, BaseMarketExt,
};

/// A market with borrowing fees.
pub trait BorrowingFeeMarket<const DECIMALS: u8>: BaseMarket<DECIMALS> {
    /// Get borrowing factor pool.
    fn borrowing_factor_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get total borrowing pool.
    fn total_borrowing_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get borrowing fee params.
    fn borrowing_fee_params(&self) -> crate::Result<BorrowingFeeParams<Self::Num>>;

    /// Get the passed time in seconds for the given kind of clock.
    fn passed_in_seconds_for_borrowing(&self) -> crate::Result<u64>;
}

impl<'a, M: BorrowingFeeMarket<DECIMALS>, const DECIMALS: u8> BorrowingFeeMarket<DECIMALS>
    for &'a mut M
{
    fn borrowing_factor_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).borrowing_factor_pool()
    }

    fn total_borrowing_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).total_borrowing_pool()
    }

    fn borrowing_fee_params(&self) -> crate::Result<BorrowingFeeParams<Self::Num>> {
        (**self).borrowing_fee_params()
    }

    fn passed_in_seconds_for_borrowing(&self) -> crate::Result<u64> {
        (**self).passed_in_seconds_for_borrowing()
    }
}

/// Extension trait for [`BorrowingFeeMarket`].
pub trait BorrowingFeeMarketExt<const DECIMALS: u8>: BorrowingFeeMarket<DECIMALS> {
    /// Get current borrowing factor.
    #[inline]
    fn cumulative_borrowing_factor(&self, is_long: bool) -> crate::Result<Self::Num> {
        self.borrowing_factor_pool()?.amount(is_long)
    }

    /// Get borrowing factor per second.
    fn borrowing_factor_per_second(
        &self,
        is_long: bool,
        prices: &Prices<Self::Num>,
    ) -> crate::Result<Self::Num> {
        use crate::utils;

        let reserved_value = self.reserved_value(&prices.index_token_price, is_long)?;

        if reserved_value.is_zero() {
            return Ok(Zero::zero());
        }

        let params = self.borrowing_fee_params()?;

        if params.skip_borrowing_fee_for_smaller_side() {
            let open_interest = self.open_interest()?;
            let long_interest = open_interest.long_amount()?;
            let short_interest = open_interest.short_amount()?;
            if (is_long && long_interest < short_interest)
                || (!is_long && short_interest < long_interest)
            {
                return Ok(Zero::zero());
            }
        }

        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, false)?;

        if pool_value.is_zero() {
            return Err(crate::Error::UnableToGetBorrowingFactorEmptyPoolValue);
        }

        // TODO: apply optimal usage factor.

        let reserved_value_after_exponent =
            utils::apply_exponent_factor(reserved_value, params.exponent(is_long).clone()).ok_or(
                crate::Error::Computation("calculating reserved value after exponent"),
            )?;
        let reversed_value_to_pool_factor =
            utils::div_to_factor(&reserved_value_after_exponent, &pool_value, false).ok_or(
                crate::Error::Computation("calculating reserved value to pool factor"),
            )?;
        utils::apply_factor(&reversed_value_to_pool_factor, params.factor(is_long)).ok_or(
            crate::Error::Computation("calculating borrowing factor per second"),
        )
    }

    /// Get next cumulative borrowing factor of the given side.
    fn next_cumulative_borrowing_factor(
        &self,
        is_long: bool,
        prices: &Prices<Self::Num>,
        duration_in_second: u64,
    ) -> crate::Result<(Self::Num, Self::Num)> {
        use num_traits::{CheckedMul, FromPrimitive};

        let borrowing_factor_per_second = self.borrowing_factor_per_second(is_long, prices)?;
        let current_factor = self.cumulative_borrowing_factor(is_long)?;

        let duration_value =
            Self::Num::from_u64(duration_in_second).ok_or(crate::Error::Convert)?;
        let delta = borrowing_factor_per_second
            .checked_mul(&duration_value)
            .ok_or(crate::Error::Computation(
                "calculating borrowing factor delta",
            ))?;
        let next_cumulative_borrowing_factor =
            current_factor
                .checked_add(&delta)
                .ok_or(crate::Error::Computation(
                    "calculating next borrowing factor",
                ))?;
        Ok((next_cumulative_borrowing_factor, delta))
    }

    /// Get total pending borrowing fees.
    fn total_pending_borrowing_fees(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
    ) -> crate::Result<Self::Num> {
        let open_interest = self.open_interest()?.amount(is_long)?;

        let duration_in_second = self.passed_in_seconds_for_borrowing()?;
        let next_cumulative_borrowing_factor = self
            .next_cumulative_borrowing_factor(is_long, prices, duration_in_second)?
            .0;
        let total_borrowing = self.total_borrowing_pool()?.amount(is_long)?;

        crate::utils::apply_factor(&open_interest, &next_cumulative_borrowing_factor)
            .and_then(|total| total.checked_sub(&total_borrowing))
            .ok_or(crate::Error::Computation(
                "calculating total pending borrowing fees",
            ))
    }
}

impl<M: BorrowingFeeMarket<DECIMALS> + ?Sized, const DECIMALS: u8> BorrowingFeeMarketExt<DECIMALS>
    for M
{
}
