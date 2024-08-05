use crate::{
    action::{
        update_borrowing_state::UpdateBorrowingState, update_funding_state::UpdateFundingState,
        Prices,
    },
    params::{
        fee::{BorrowingFeeParams, FundingFeeParams},
        FeeParams, PositionParams,
    },
    BalanceExt, PoolExt, PositionImpactMarket, PositionImpactMarketMut, SwapMarket, SwapMarketMut,
};

use super::BaseMarketExt;

/// A perpetual market.
pub trait PerpMarket<const DECIMALS: u8>:
    SwapMarket<DECIMALS> + PositionImpactMarket<DECIMALS>
{
    /// Get funding factor per second.
    fn funding_factor_per_second(&self) -> &Self::Signed;

    /// Get borrowing factor pool.
    fn borrowing_factor_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get funding amount per size pool.
    fn funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get claimable funding amount per size pool.
    fn claimable_funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get total borrowing pool.
    fn total_borrowing_pool(&self) -> crate::Result<&Self::Pool>;

    /// Get borrowing fee params.
    fn borrowing_fee_params(&self) -> crate::Result<BorrowingFeeParams<Self::Num>>;

    /// Adjustment factor for packing funding amount per size.
    fn funding_amount_per_size_adjustment(&self) -> Self::Num;

    /// Get funding fee params.
    fn funding_fee_params(&self) -> crate::Result<FundingFeeParams<Self::Num>>;

    /// Get basic position params.
    fn position_params(&self) -> crate::Result<PositionParams<Self::Num>>;

    /// Get the order fee params.
    fn order_fee_params(&self) -> crate::Result<FeeParams<Self::Num>>;

    /// Get open interest reserve factor.
    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num>;

    /// Get max open interest.
    fn max_open_interest(&self, is_long: bool) -> crate::Result<Self::Num>;
}

/// A mutable perpetual market.
pub trait PerpMarketMut<const DECIMALS: u8>:
    SwapMarketMut<DECIMALS> + PositionImpactMarketMut<DECIMALS> + PerpMarket<DECIMALS>
{
    /// Get the just passed time in seconds for the given kind of clock.
    fn just_passed_in_seconds_for_borrowing(&mut self) -> crate::Result<u64>;

    /// Get the just passed time in seconds for the given kind of clock.
    fn just_passed_in_seconds_for_funding(&mut self) -> crate::Result<u64>;

    /// Get funding factor per second mutably.
    fn funding_factor_per_second_mut(&mut self) -> &mut Self::Signed;

    /// Get mutable reference of open interest pool.
    fn open_interest_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool>;

    /// Get mutable reference of open interest pool.
    fn open_interest_in_tokens_pool_mut(&mut self, is_long: bool)
        -> crate::Result<&mut Self::Pool>;

    /// Get borrowing factor pool mutably.
    fn borrowing_factor_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Get funding amount per size pool mutably.
    fn funding_amount_per_size_pool_mut(&mut self, is_long: bool)
        -> crate::Result<&mut Self::Pool>;

    /// Get claimable funding amount per size pool mutably.
    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool>;

    /// Get collateral sum pool mutably.
    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool>;

    /// Get total borrowing pool mutably.
    fn total_borrowing_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;
}

impl<'a, M: PerpMarket<DECIMALS>, const DECIMALS: u8> PerpMarket<DECIMALS> for &'a mut M {
    fn funding_factor_per_second(&self) -> &Self::Signed {
        (**self).funding_factor_per_second()
    }

    fn borrowing_fee_params(&self) -> crate::Result<BorrowingFeeParams<Self::Num>> {
        (**self).borrowing_fee_params()
    }

    fn borrowing_factor_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).borrowing_factor_pool()
    }

    fn funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).funding_amount_per_size_pool(is_long)
    }

    fn claimable_funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).claimable_funding_amount_per_size_pool(is_long)
    }

    fn total_borrowing_pool(&self) -> crate::Result<&Self::Pool> {
        (**self).total_borrowing_pool()
    }

    fn funding_amount_per_size_adjustment(&self) -> Self::Num {
        (**self).funding_amount_per_size_adjustment()
    }

    fn funding_fee_params(&self) -> crate::Result<FundingFeeParams<Self::Num>> {
        (**self).funding_fee_params()
    }

    fn position_params(&self) -> crate::Result<PositionParams<Self::Num>> {
        (**self).position_params()
    }

    fn order_fee_params(&self) -> crate::Result<FeeParams<Self::Num>> {
        (**self).order_fee_params()
    }

    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num> {
        (**self).open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> crate::Result<Self::Num> {
        (**self).max_open_interest(is_long)
    }
}

impl<'a, M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> PerpMarketMut<DECIMALS> for &'a mut M {
    fn funding_factor_per_second_mut(&mut self) -> &mut Self::Signed {
        (**self).funding_factor_per_second_mut()
    }

    fn open_interest_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool> {
        (**self).open_interest_pool_mut(is_long)
    }

    fn open_interest_in_tokens_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        (**self).open_interest_in_tokens_pool_mut(is_long)
    }

    fn borrowing_factor_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).borrowing_factor_pool_mut()
    }

    fn funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        (**self).funding_amount_per_size_pool_mut(is_long)
    }

    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        (**self).claimable_funding_amount_per_size_pool_mut(is_long)
    }

    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool> {
        (**self).collateral_sum_pool_mut(is_long)
    }

    fn total_borrowing_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        (**self).total_borrowing_pool_mut()
    }

    fn just_passed_in_seconds_for_borrowing(&mut self) -> crate::Result<u64> {
        (**self).just_passed_in_seconds_for_borrowing()
    }

    fn just_passed_in_seconds_for_funding(&mut self) -> crate::Result<u64> {
        (**self).just_passed_in_seconds_for_funding()
    }
}

/// Extension trait for [`PerpMarket`].
pub trait PerpMarketExt<const DECIMALS: u8>: PerpMarket<DECIMALS> {
    /// Get current borrowing factor.
    #[inline]
    fn borrowing_factor(&self, is_long: bool) -> crate::Result<Self::Num> {
        self.borrowing_factor_pool()?.amount(is_long)
    }

    /// Get current funding fee amount per size.
    #[inline]
    fn funding_fee_amount_per_size(
        &self,
        is_long: bool,
        is_long_collateral: bool,
    ) -> crate::Result<Self::Num> {
        self.funding_amount_per_size_pool(is_long)?
            .amount(is_long_collateral)
    }

    /// Get current claimable funding fee amount per size.
    #[inline]
    fn claimable_funding_fee_amount_per_size(
        &self,
        is_long: bool,
        is_long_collateral: bool,
    ) -> crate::Result<Self::Num> {
        self.claimable_funding_amount_per_size_pool(is_long)?
            .amount(is_long_collateral)
    }

    /// Validate open interest reserve.
    fn validate_open_interest_reserve(
        &self,
        prices: &Prices<Self::Num>,
        is_long: bool,
    ) -> crate::Result<()> {
        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long, false)?;

        let max_reserved_value =
            crate::utils::apply_factor(&pool_value, &self.open_interest_reserve_factor()?)
                .ok_or(crate::Error::Computation("calculating max reserved value"))?;

        let reserved_value = self.reserved_value(&prices.index_token_price, is_long)?;

        if reserved_value > max_reserved_value {
            Err(crate::Error::InsufficientReserveForOpenInterest)
        } else {
            Ok(())
        }
    }
}

impl<M: PerpMarket<DECIMALS>, const DECIMALS: u8> PerpMarketExt<DECIMALS> for M {}

/// Extension trait for [`PerpMarket`].
pub trait PerpMarketMutExt<const DECIMALS: u8>: PerpMarketMut<DECIMALS> {
    /// Create a [`UpdateBorrowingState`] action.
    fn update_borrowing(
        &mut self,
        prices: &Prices<Self::Num>,
    ) -> crate::Result<UpdateBorrowingState<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        UpdateBorrowingState::try_new(self, prices)
    }

    /// Create a [`UpdateFundingState`] action.
    fn update_funding(
        &mut self,
        prices: &Prices<Self::Num>,
    ) -> crate::Result<UpdateFundingState<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        UpdateFundingState::try_new(self, prices)
    }

    /// Apply delta to funding amount per size.
    fn apply_delta_to_funding_amount_per_size(
        &mut self,
        is_long: bool,
        is_long_collateral: bool,
        delta: &Self::Signed,
    ) -> crate::Result<()> {
        self.funding_amount_per_size_pool_mut(is_long)?
            .apply_delta_amount(is_long_collateral, delta)
    }

    /// Apply delta to claimable funding amount per size.
    fn apply_delta_to_claimable_funding_amount_per_size(
        &mut self,
        is_long: bool,
        is_long_collateral: bool,
        delta: &Self::Signed,
    ) -> crate::Result<()> {
        self.claimable_funding_amount_per_size_pool_mut(is_long)?
            .apply_delta_amount(is_long_collateral, delta)
    }
}

impl<M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> PerpMarketMutExt<DECIMALS> for M {}
