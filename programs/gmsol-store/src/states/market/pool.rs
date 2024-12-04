use anchor_lang::prelude::*;
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
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pool {
    /// Whether the pool only contains one kind of token,
    /// i.e. a pure pool.
    /// For a pure pool, only the `long_token_amount` field is used.
    is_pure: u8,
    #[cfg_attr(feature = "serde", serde(skip))]
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
}

/// Market Pools.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
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
    /// Point pool.
    point: PoolStorage,
    reserved: [PoolStorage; 4],
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
        // Point pool must be impure.
        self.point.set_is_pure(false);
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
            PoolKind::Point => &self.point,
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
            PoolKind::Point => &mut self.point,
            _ => return None,
        };
        Some(pool)
    }
}
