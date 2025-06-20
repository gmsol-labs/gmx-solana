use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use gmsol_model::PoolKind;

/// A pool storage for market.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PoolStorage {
    pub(super) rev: u64,
    padding: [u8; 8],
    pool: Pool,
}

impl PoolStorage {
    /// Set the pure flag.
    pub(crate) fn set_is_pure(&mut self, is_pure: bool) {
        self.pool.set_is_pure(is_pure);
    }

    /// Get pool.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Get pool mutably.
    pub(super) fn pool_mut(&mut self) -> &mut Pool {
        &mut self.pool
    }
}

/// A pool for market.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace, PartialEq, Eq)]
pub struct Pool {
    /// Whether the pool only contains one kind of token,
    /// i.e. a pure pool.
    /// For a pure pool, only the `long_token_amount` field is used.
    is_pure: u8,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 15],
    /// Long token amount.
    pub(super) long_token_amount: u128,
    /// Short token amount.
    pub(super) short_token_amount: u128,
}

const PURE_VALUE: u8 = 1;

impl Pool {
    /// Set the pure flag.
    fn set_is_pure(&mut self, is_pure: bool) {
        self.is_pure = if is_pure { PURE_VALUE } else { 0 };
    }

    /// Is this a pure pool.
    fn is_pure(&self) -> bool {
        !matches!(self.is_pure, 0)
    }
}

impl gmsol_model::Balance for Pool {
    type Num = u128;

    type Signed = i128;

    /// Get the long token amount.
    fn long_amount(&self) -> gmsol_model::Result<Self::Num> {
        if self.is_pure() {
            debug_assert_eq!(
                self.short_token_amount, 0,
                "short token amount must be zero"
            );
            // For pure pools, we must ensure that the long token amount
            // plus the short token amount equals the total token amount.
            // Therefore, we use `div_ceil` for the long token amount
            // and `div` for the short token amount.
            Ok(self.long_token_amount.div_ceil(2))
        } else {
            Ok(self.long_token_amount)
        }
    }

    /// Get the short token amount.
    fn short_amount(&self) -> gmsol_model::Result<Self::Num> {
        if self.is_pure() {
            debug_assert_eq!(
                self.short_token_amount, 0,
                "short token amount must be zero"
            );
            Ok(self.long_token_amount / 2)
        } else {
            Ok(self.short_token_amount)
        }
    }
}

impl gmsol_model::Pool for Pool {
    fn apply_delta_to_long_amount(&mut self, delta: &Self::Signed) -> gmsol_model::Result<()> {
        self.long_token_amount = self.long_token_amount.checked_add_signed(*delta).ok_or(
            gmsol_model::Error::Computation("apply delta to long amount"),
        )?;
        Ok(())
    }

    fn apply_delta_to_short_amount(&mut self, delta: &Self::Signed) -> gmsol_model::Result<()> {
        let amount = if self.is_pure() {
            &mut self.long_token_amount
        } else {
            &mut self.short_token_amount
        };
        *amount = amount
            .checked_add_signed(*delta)
            .ok_or(gmsol_model::Error::Computation(
                "apply delta to short amount",
            ))?;
        Ok(())
    }

    fn checked_apply_delta(
        &self,
        delta: gmsol_model::Delta<&Self::Signed>,
    ) -> gmsol_model::Result<Self> {
        let mut ans = *self;
        if let Some(amount) = delta.long() {
            ans.apply_delta_to_long_amount(amount)?;
        }
        if let Some(amount) = delta.short() {
            ans.apply_delta_to_short_amount(amount)?;
        }
        Ok(ans)
    }

    fn checked_cancel_amounts(&self) -> gmsol_model::Result<Self>
    where
        Self::Signed: gmsol_model::num_traits::CheckedSub,
    {
        let mut ans = *self;
        if self.is_pure() {
            ans.long_token_amount &= 1;
        } else {
            (ans.long_token_amount, ans.short_token_amount) =
                cancel_amounts(ans.long_token_amount, ans.short_token_amount);
        }

        Ok(ans)
    }
}

pub(crate) fn cancel_amounts(long_amount: u128, short_amount: u128) -> (u128, u128) {
    let is_long_side_left = long_amount >= short_amount;
    let leftover_amount = long_amount.abs_diff(short_amount);
    if is_long_side_left {
        (leftover_amount, 0)
    } else {
        (0, leftover_amount)
    }
}

/// Market Pools.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pools {
    /// Primary Pool.
    primary: PoolStorage,
    /// Swap Impact Pool.
    swap_impact: PoolStorage,
    /// Claimable Fee Pool.
    claimable_fee: PoolStorage,
    /// Long open interest.
    open_interest_for_long: PoolStorage,
    /// Short open interest.
    open_interest_for_short: PoolStorage,
    /// Long open interest in tokens.
    open_interest_in_tokens_for_long: PoolStorage,
    /// Short open interest in tokens.
    open_interest_in_tokens_for_short: PoolStorage,
    /// Position Impact.
    position_impact: PoolStorage,
    /// Borrowing Factor.
    borrowing_factor: PoolStorage,
    /// Funding Amount Per Size for long.
    funding_amount_per_size_for_long: PoolStorage,
    /// Funding Amount Per Size for short.
    funding_amount_per_size_for_short: PoolStorage,
    /// Claimable Funding Amount Per Size for long.
    claimable_funding_amount_per_size_for_long: PoolStorage,
    /// Claimable Funding Amount Per Size for short.
    claimable_funding_amount_per_size_for_short: PoolStorage,
    /// Collateral sum pool for long.
    collateral_sum_for_long: PoolStorage,
    /// Collateral sum pool for short.
    collateral_sum_for_short: PoolStorage,
    /// Total borrowing pool.
    total_borrowing: PoolStorage,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [PoolStorage; 16],
}

impl Pools {
    pub(super) fn init(&mut self, is_pure: bool) {
        self.primary.set_is_pure(is_pure);
        self.swap_impact.set_is_pure(is_pure);
        self.claimable_fee.set_is_pure(is_pure);
        self.open_interest_for_long.set_is_pure(is_pure);
        self.open_interest_for_short.set_is_pure(is_pure);
        self.open_interest_in_tokens_for_long.set_is_pure(is_pure);
        self.open_interest_in_tokens_for_short.set_is_pure(is_pure);
        // Position impact pool must be impure.
        self.position_impact.set_is_pure(false);
        // Borrowing factor must be impure.
        self.borrowing_factor.set_is_pure(false);
        self.funding_amount_per_size_for_long.set_is_pure(is_pure);
        self.funding_amount_per_size_for_short.set_is_pure(is_pure);
        self.claimable_funding_amount_per_size_for_long
            .set_is_pure(is_pure);
        self.claimable_funding_amount_per_size_for_short
            .set_is_pure(is_pure);
        self.collateral_sum_for_long.set_is_pure(is_pure);
        self.collateral_sum_for_short.set_is_pure(is_pure);
        // Total borrowing pool must be impure.
        self.total_borrowing.set_is_pure(false);
    }

    pub(super) fn get(&self, kind: PoolKind) -> Option<&PoolStorage> {
        let pool = match kind {
            PoolKind::Primary => &self.primary,
            PoolKind::SwapImpact => &self.swap_impact,
            PoolKind::ClaimableFee => &self.claimable_fee,
            PoolKind::OpenInterestForLong => &self.open_interest_for_long,
            PoolKind::OpenInterestForShort => &self.open_interest_for_short,
            PoolKind::OpenInterestInTokensForLong => &self.open_interest_in_tokens_for_long,
            PoolKind::OpenInterestInTokensForShort => &self.open_interest_in_tokens_for_short,
            PoolKind::PositionImpact => &self.position_impact,
            PoolKind::BorrowingFactor => &self.borrowing_factor,
            PoolKind::FundingAmountPerSizeForLong => &self.funding_amount_per_size_for_long,
            PoolKind::FundingAmountPerSizeForShort => &self.funding_amount_per_size_for_short,
            PoolKind::ClaimableFundingAmountPerSizeForLong => {
                &self.claimable_funding_amount_per_size_for_long
            }
            PoolKind::ClaimableFundingAmountPerSizeForShort => {
                &self.claimable_funding_amount_per_size_for_short
            }
            PoolKind::CollateralSumForLong => &self.collateral_sum_for_long,
            PoolKind::CollateralSumForShort => &self.collateral_sum_for_short,
            PoolKind::TotalBorrowing => &self.total_borrowing,
            _ => return None,
        };
        Some(pool)
    }

    pub(super) fn get_mut(&mut self, kind: PoolKind) -> Option<&mut PoolStorage> {
        let pool = match kind {
            PoolKind::Primary => &mut self.primary,
            PoolKind::SwapImpact => &mut self.swap_impact,
            PoolKind::ClaimableFee => &mut self.claimable_fee,
            PoolKind::OpenInterestForLong => &mut self.open_interest_for_long,
            PoolKind::OpenInterestForShort => &mut self.open_interest_for_short,
            PoolKind::OpenInterestInTokensForLong => &mut self.open_interest_in_tokens_for_long,
            PoolKind::OpenInterestInTokensForShort => &mut self.open_interest_in_tokens_for_short,
            PoolKind::PositionImpact => &mut self.position_impact,
            PoolKind::BorrowingFactor => &mut self.borrowing_factor,
            PoolKind::FundingAmountPerSizeForLong => &mut self.funding_amount_per_size_for_long,
            PoolKind::FundingAmountPerSizeForShort => &mut self.funding_amount_per_size_for_short,
            PoolKind::ClaimableFundingAmountPerSizeForLong => {
                &mut self.claimable_funding_amount_per_size_for_long
            }
            PoolKind::ClaimableFundingAmountPerSizeForShort => {
                &mut self.claimable_funding_amount_per_size_for_short
            }
            PoolKind::CollateralSumForLong => &mut self.collateral_sum_for_long,
            PoolKind::CollateralSumForShort => &mut self.collateral_sum_for_short,
            PoolKind::TotalBorrowing => &mut self.total_borrowing,
            _ => return None,
        };
        Some(pool)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::events::EventPool;

    #[test]
    fn test_event_pool() {
        let pool = Pool {
            is_pure: PURE_VALUE,
            padding: Default::default(),
            long_token_amount: u128::MAX,
            short_token_amount: u128::MAX,
        };

        let event_pool = EventPool {
            is_pure: pool.is_pure,
            padding: pool.padding,
            long_token_amount: pool.long_token_amount,
            short_token_amount: pool.short_token_amount,
        };

        let mut data = Vec::with_capacity(Pool::INIT_SPACE);
        pool.serialize(&mut data)
            .expect("failed to serialize `Pool`");

        let mut event_data = Vec::with_capacity(Pool::INIT_SPACE);
        event_pool
            .serialize(&mut event_data)
            .expect("failed to serialize `EventPool`");

        assert_eq!(data, event_data);
    }

    #[cfg(feature = "debug")]
    #[test]
    fn cancel_amounts() -> gmsol_model::Result<()> {
        use bytemuck::Zeroable;
        use gmsol_model::{Balance, Delta, Pool};

        let pool = super::Pool::zeroed();

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
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &i128::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &i128::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &1))?;
        let expected_4 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &0))?;
        assert_eq!(pool_4.long_amount()?, u128::MAX);
        assert_eq!(pool_4.short_amount()?, u128::MAX);
        assert_eq!(pool_4.checked_cancel_amounts()?, expected_4);

        let pool_5 =
            pool.checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &i128::MAX))?;
        let expected_5 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &0))?;
        assert_eq!(pool_5.checked_cancel_amounts()?, expected_5);
        let pool_5 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &i128::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &0))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        let expected_5 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &0))?
            .checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        assert_eq!(pool_5.long_amount()?, u128::MAX);
        assert_eq!(pool_5.short_amount()?, i128::MAX.unsigned_abs());
        assert_eq!(pool_5.checked_cancel_amounts()?, expected_5);

        Ok(())
    }

    #[cfg(feature = "debug")]
    #[test]
    fn cancel_amounts_for_pure_pools() -> gmsol_model::Result<()> {
        use bytemuck::Zeroable;
        use gmsol_model::{Balance, Delta, Pool};

        let mut pool = super::Pool::zeroed();
        pool.set_is_pure(true);

        let pool_1 = pool.checked_apply_delta(Delta::new_both_sides(true, &1_001, &3_000))?;
        let expected_1 = pool.checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        assert_eq!(pool_1.checked_cancel_amounts()?, expected_1);

        let pool_2 = pool.checked_apply_delta(Delta::new_both_sides(true, &3_005, &3005))?;
        let expected_2 = pool.checked_apply_delta(Delta::new_both_sides(true, &0, &0))?;
        assert_eq!(pool_2.checked_cancel_amounts()?, expected_2);

        let pool_3 = pool
            .checked_apply_delta(Delta::new_both_sides(true, &i128::MAX, &i128::MAX))?
            .checked_apply_delta(Delta::new_both_sides(true, &0, &1))?;
        let expected_3 = pool.checked_apply_delta(Delta::new_both_sides(true, &1, &0))?;
        assert_eq!(pool_3.long_amount()?, u128::MAX / 2 + 1);
        assert_eq!(pool_3.short_amount()?, u128::MAX / 2);
        assert_eq!(pool_3.checked_cancel_amounts()?, expected_3);

        Ok(())
    }
}
