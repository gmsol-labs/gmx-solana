use num_traits::CheckedSub;

use crate::num::Unsigned;

use self::delta::PoolDelta;

/// Balance.
pub mod balance;

/// Delta.
pub mod delta;

pub use self::{
    balance::{Balance, BalanceExt},
    delta::Delta,
};

/// A balance for holding tokens, usd values, or factors
pub trait Pool: Balance + Sized {
    /// Apply delta to long amount.
    fn apply_delta_to_long_amount(&mut self, delta: &Self::Signed) -> crate::Result<()> {
        *self = self.checked_apply_delta(Delta::new_with_long(delta))?;
        Ok(())
    }

    /// Apply delta to short amount.
    fn apply_delta_to_short_amount(&mut self, delta: &Self::Signed) -> crate::Result<()> {
        *self = self.checked_apply_delta(Delta::new_with_short(delta))?;
        Ok(())
    }

    /// Checked apply delta amounts.
    fn checked_apply_delta(&self, delta: Delta<&Self::Signed>) -> crate::Result<Self>;

    /// Cancel the amounts, for example, (1000, 200) -> (800, 0).
    /// # Warning
    /// - Note that the default implementation may lead to unnecessary
    ///   failures due to the numeric range limitations of signed integers.
    fn checked_cancel_amounts(&self) -> crate::Result<Self>
    where
        Self::Signed: CheckedSub,
    {
        let long_amount = self.long_amount()?;
        let short_amount = self.short_amount()?;
        let is_long_side_left = long_amount >= short_amount;
        let leftover_amount = long_amount.clone().diff(short_amount.clone());
        let (long_delta, short_delta) = if is_long_side_left {
            (long_amount.diff(leftover_amount), short_amount)
        } else {
            (long_amount, short_amount.diff(leftover_amount))
        };
        self.checked_apply_delta(Delta::new_both_sides(
            true,
            &long_delta.to_opposite_signed()?,
            &short_delta.to_opposite_signed()?,
        ))
    }
}

/// Extension trait for [`Pool`] with utils.
pub trait PoolExt: Pool {
    /// Apply delta.
    #[inline]
    fn apply_delta_amount(&mut self, is_long: bool, delta: &Self::Signed) -> crate::Result<()> {
        if is_long {
            self.apply_delta_to_long_amount(delta)
        } else {
            self.apply_delta_to_short_amount(delta)
        }
    }
}

impl<P: Pool> PoolExt for P {}

/// Pool kind.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[cfg_attr(
    feature = "strum",
    derive(strum::EnumIter, strum::EnumString, strum::Display)
)]
#[cfg_attr(feature = "strum", strum(serialize_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(
        anchor_lang::AnchorDeserialize,
        anchor_lang::AnchorSerialize,
        anchor_lang::InitSpace
    )
)]
#[repr(u8)]
#[non_exhaustive]
pub enum PoolKind {
    /// Primary liquidity pool.
    #[default]
    Primary,
    /// Swap impact.
    SwapImpact,
    /// Claimable fee.
    ClaimableFee,
    /// Open Interest for long.
    OpenInterestForLong,
    /// Open Interest for short.
    OpenInterestForShort,
    /// Open Interest in tokens for long.
    OpenInterestInTokensForLong,
    /// Open Interest in tokens for short.
    OpenInterestInTokensForShort,
    /// Position impact.
    PositionImpact,
    /// Borrowing factor.
    BorrowingFactor,
    /// Funding amount per size for long.
    FundingAmountPerSizeForLong,
    /// Funding amount per size for short.
    FundingAmountPerSizeForShort,
    /// Claimable funding amount per size for long.
    ClaimableFundingAmountPerSizeForLong,
    /// Claimable funding amount per size for short.
    ClaimableFundingAmountPerSizeForShort,
    /// Collateral sum for long.
    CollateralSumForLong,
    /// Collateral sum for short.
    CollateralSumForShort,
    /// Total borrowing.
    TotalBorrowing,
}

#[cfg(test)]
mod tests {
    use crate::{test::TestPool, Balance};

    use super::{Delta, Pool};

    #[test]
    fn cancel_amounts() -> crate::Result<()> {
        let pool = TestPool::<u64>::default();

        let pool_1 = pool.checked_apply_delta(Delta::new_both_sides(true, &1_000, &3_000))?;
        let expected_1 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &2_000))?;
        assert_eq!(pool_1.checked_cancel_amounts()?, expected_1);

        let pool_2 = pool.checked_apply_delta(Delta::new_both_sides(true, &3_005, &3_000))?;
        let expected_2 = pool.checked_apply_delta(Delta::new_both_sides(true, &5, &0))?;
        assert_eq!(pool_2.checked_cancel_amounts()?, expected_2);

        let pool_3 = pool.checked_apply_delta(Delta::new_both_sides(true, &3_000, &3_000))?;
        let expected_3 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &0))?;
        assert_eq!(pool_3.checked_cancel_amounts()?, expected_3);

        let pool_4 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &i64::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &i64::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &1))?;
        assert_eq!(pool_4.long_amount()?, u64::MAX);
        assert_eq!(pool_4.short_amount()?, u64::MAX);
        // Overflow occurs due to the limitations of the default implementation.
        assert!(pool_4.checked_cancel_amounts().is_err());

        let pool_5 = pool.checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &i64::MAX))?;
        let expected_5 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &0))?;
        assert_eq!(pool_5.checked_cancel_amounts()?, expected_5);
        let pool_5 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &i64::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &0))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        let expected_5 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i64::MAX, &0))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        assert_eq!(pool_5.long_amount()?, u64::MAX);
        assert_eq!(pool_5.short_amount()?, i64::MAX.unsigned_abs());
        assert_eq!(pool_5.checked_cancel_amounts()?, expected_5);

        Ok(())
    }
}
