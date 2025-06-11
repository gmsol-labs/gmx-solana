use std::cmp::Ordering;

use num_traits::{CheckedAdd, CheckedMul, CheckedNeg, CheckedSub, Zero};

use crate::{
    fixed::FixedPointOps, num::Unsigned, params::PriceImpactParams, utils, Balance, BalanceExt,
};

/// Represents the change of balance.
#[derive(Debug, Clone, Copy)]
pub enum BalanceChange {
    /// Balance was improved.
    Improved,
    /// Balance was worsened.
    Worsened,
    /// Balance was unchanged.
    Unchanged,
}

/// Represents the effect caused by balance change.
#[derive(Debug, Clone, Copy)]
pub struct PriceImpact<T> {
    /// Price impact value.
    pub value: T,
    /// Balance change.
    pub balance_change: BalanceChange,
}

impl<T: Zero> Default for PriceImpact<T> {
    fn default() -> Self {
        Self {
            value: Zero::zero(),
            balance_change: BalanceChange::Unchanged,
        }
    }
}

/// Delta Amounts.
#[derive(Debug, Clone, Copy)]
pub struct Delta<T> {
    /// Long amount.
    long: Option<T>,
    /// Short amount.
    short: Option<T>,
}

impl<T> Delta<T> {
    /// Create a new delta amounts.
    pub fn new(long: Option<T>, short: Option<T>) -> Self {
        Self { long, short }
    }

    /// Create a long delta amount.
    #[inline]
    pub fn new_with_long(amount: T) -> Self {
        Self::new(Some(amount), None)
    }

    /// Create a short delta amount.
    #[inline]
    pub fn new_with_short(amount: T) -> Self {
        Self::new(None, Some(amount))
    }

    /// Create a delta amount for one side.
    #[inline]
    pub fn new_one_side(is_long: bool, amount: T) -> Self {
        if is_long {
            Self::new_with_long(amount)
        } else {
            Self::new_with_short(amount)
        }
    }

    /// Create delta amounts for both sides.
    #[inline]
    pub fn new_both_sides(is_long_first: bool, first: T, second: T) -> Self {
        let (long, short) = if is_long_first {
            (first, second)
        } else {
            (second, first)
        };
        Self::new(Some(long), Some(short))
    }

    /// Get long delta amount.
    pub fn long(&self) -> Option<&T> {
        self.long.as_ref()
    }

    /// Get short delta amount.
    pub fn short(&self) -> Option<&T> {
        self.short.as_ref()
    }
}

/// Usd values of pool.
pub struct PoolValue<T> {
    long_token_usd_value: T,
    short_token_usd_value: T,
}

impl<T> PoolValue<T> {
    /// Get long token usd value.
    pub fn long_value(&self) -> &T {
        &self.long_token_usd_value
    }

    /// Get short token usd value.
    pub fn short_value(&self) -> &T {
        &self.short_token_usd_value
    }
}

impl<T: Unsigned + Clone> PoolValue<T> {
    /// Get usd value (abs) difference.
    #[inline]
    pub fn diff_value(&self) -> T {
        self.long_token_usd_value
            .clone()
            .diff(self.short_token_usd_value.clone())
    }
}

impl<T: Unsigned> PoolValue<T> {
    /// Create a new [`PoolValue`] from the given pool and prices.
    pub fn try_new<P>(pool: &P, long_token_price: &T, short_token_price: &T) -> crate::Result<Self>
    where
        P: Balance<Num = T, Signed = T::Signed> + ?Sized,
    {
        let long_token_usd_value = pool.long_usd_value(long_token_price)?;
        let short_token_usd_value = pool.short_usd_value(short_token_price)?;
        Ok(Self {
            long_token_usd_value,
            short_token_usd_value,
        })
    }
}

/// Delta of pool usd values.
pub struct PoolDelta<T: Unsigned> {
    current: PoolValue<T>,
    next: PoolValue<T>,
    delta: PoolValue<T::Signed>,
    long_token_price: T,
    short_token_price: T,
}

impl<T: Unsigned> PoolDelta<T> {
    /// Create a new [`PoolDelta`].
    pub fn try_new<P>(
        pool: &P,
        delta_long_token_usd_value: T::Signed,
        delta_short_token_usd_value: T::Signed,
        long_token_price: &T,
        short_token_price: &T,
    ) -> crate::Result<Self>
    where
        T: CheckedAdd + CheckedSub + CheckedMul + Clone,
        P: Balance<Num = T, Signed = T::Signed> + ?Sized,
    {
        let current = PoolValue::try_new(pool, long_token_price, short_token_price)?;

        let next = PoolValue {
            long_token_usd_value: current
                .long_token_usd_value
                .checked_add_with_signed(&delta_long_token_usd_value)
                .ok_or(crate::Error::Computation("next delta long usd value"))?,
            short_token_usd_value: current
                .short_token_usd_value
                .checked_add_with_signed(&delta_short_token_usd_value)
                .ok_or(crate::Error::Computation("next delta short usd value"))?,
        };

        let delta = PoolValue {
            long_token_usd_value: delta_long_token_usd_value,
            short_token_usd_value: delta_short_token_usd_value,
        };

        Ok(Self {
            current,
            next,
            delta,
            long_token_price: long_token_price.clone(),
            short_token_price: short_token_price.clone(),
        })
    }

    /// Create a new [`PoolDelta`].
    pub fn try_from_delta_amounts<P>(
        pool: &P,
        long_delta_amount: &T::Signed,
        short_delta_amount: &T::Signed,
        long_token_price: &T,
        short_token_price: &T,
    ) -> crate::Result<Self>
    where
        T: CheckedAdd + CheckedSub + CheckedMul + Clone,
        P: Balance<Num = T, Signed = T::Signed> + ?Sized,
    {
        let delta_long_token_usd_value = long_token_price
            .checked_mul_with_signed(long_delta_amount)
            .ok_or(crate::Error::Computation("delta long token usd value"))?;
        let delta_short_token_usd_value = short_token_price
            .checked_mul_with_signed(short_delta_amount)
            .ok_or(crate::Error::Computation("delta short token usd value"))?;
        Self::try_new(
            pool,
            delta_long_token_usd_value,
            delta_short_token_usd_value,
            long_token_price,
            short_token_price,
        )
    }

    /// Get delta values.
    pub fn delta(&self) -> &PoolValue<T::Signed> {
        &self.delta
    }

    /// Returns long token price.
    pub fn long_token_price(&self) -> &T {
        &self.long_token_price
    }

    /// Returns short token price.
    pub fn short_token_price(&self) -> &T {
        &self.short_token_price
    }
}

impl<T: Unsigned + Clone + Ord> PoolDelta<T> {
    /// Initial diff usd value.
    #[inline]
    pub fn initial_diff_value(&self) -> T {
        self.current.diff_value()
    }

    /// Next diff usd value.
    #[inline]
    pub fn next_diff_value(&self) -> T {
        self.next.diff_value()
    }

    /// Returns whether it is a same side rebalance.
    #[inline]
    pub fn is_same_side_rebalance(&self) -> bool {
        (self.current.long_token_usd_value <= self.current.short_token_usd_value)
            == (self.next.long_token_usd_value <= self.next.short_token_usd_value)
    }

    /// Calculate price impact.
    pub fn price_impact<const DECIMALS: u8>(
        &self,
        params: &PriceImpactParams<T>,
    ) -> crate::Result<PriceImpact<T::Signed>>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let initial = self.initial_diff_value();
        let next = self.next_diff_value();
        let balance_change = match next.cmp(&initial) {
            Ordering::Equal => BalanceChange::Unchanged,
            Ordering::Greater => BalanceChange::Worsened,
            Ordering::Less => BalanceChange::Improved,
        };
        let price_impact = if self.is_same_side_rebalance() {
            Self::price_impact_for_same_side_rebalance(initial, next, params)?
        } else {
            Self::price_impact_for_cross_over_rebalance(initial, next, params)?
        };
        Ok(PriceImpact {
            value: price_impact,
            balance_change,
        })
    }

    #[inline]
    fn price_impact_for_same_side_rebalance<const DECIMALS: u8>(
        initial: T,
        next: T,
        params: &PriceImpactParams<T>,
    ) -> crate::Result<T::Signed>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let has_positive_impact = next < initial;
        let (positive_factor, negative_factor) = params.adjusted_factors();

        let factor = if has_positive_impact {
            positive_factor
        } else {
            negative_factor
        };
        let exponent_factor = params.exponent();

        let initial = utils::apply_factors(initial, factor.clone(), exponent_factor.clone())?;
        let next = utils::apply_factors(next, factor.clone(), exponent_factor.clone())?;
        let delta: T::Signed = initial
            .diff(next)
            .try_into()
            .map_err(|_| crate::Error::Convert)?;
        Ok(if has_positive_impact {
            delta
        } else {
            delta.checked_neg().ok_or(crate::Error::Computation(
                "same side rebalance: negating delta",
            ))?
        })
    }

    #[inline]
    fn price_impact_for_cross_over_rebalance<const DECIMALS: u8>(
        initial: T,
        next: T,
        params: &PriceImpactParams<T>,
    ) -> crate::Result<T::Signed>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let (positive_factor, negative_factor) = params.adjusted_factors();
        let exponent_factor = params.exponent();
        let positive_impact =
            utils::apply_factors(initial, positive_factor.clone(), exponent_factor.clone())?;
        let negative_impact =
            utils::apply_factors(next, negative_factor.clone(), exponent_factor.clone())?;
        let has_positive_impact = positive_impact > negative_impact;
        let delta: T::Signed = positive_impact
            .diff(negative_impact)
            .try_into()
            .map_err(|_| crate::Error::Convert)?;
        Ok(if has_positive_impact {
            delta
        } else {
            delta.checked_neg().ok_or(crate::Error::Computation(
                "cross over rebalance: negating delta",
            ))?
        })
    }
}
