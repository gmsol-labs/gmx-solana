use crate::{
    action::{
        update_borrowing_state::UpdateBorrowingState, update_funding_state::UpdateFundingState,
    },
    num::Unsigned,
    params::{
        fee::{FundingFeeParams, LiquidationFeeParams},
        FeeParams, PositionParams,
    },
    price::{Price, Prices},
    BalanceExt, BorrowingFeeMarket, PoolExt, PositionImpactMarket, PositionImpactMarketMut,
    SwapMarket, SwapMarketMut,
};

use super::BaseMarketExt;

/// A perpetual market.
pub trait PerpMarket<const DECIMALS: u8>:
    SwapMarket<DECIMALS> + PositionImpactMarket<DECIMALS> + BorrowingFeeMarket<DECIMALS>
{
    /// Get funding factor per second.
    fn funding_factor_per_second(&self) -> &Self::Signed;

    /// Get funding amount per size pool.
    fn funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Get claimable funding amount per size pool.
    fn claimable_funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool>;

    /// Adjustment factor for packing funding amount per size.
    fn funding_amount_per_size_adjustment(&self) -> Self::Num;

    /// Get funding fee params.
    fn funding_fee_params(&self) -> crate::Result<FundingFeeParams<Self::Num>>;

    /// Get basic position params.
    fn position_params(&self) -> crate::Result<PositionParams<Self::Num>>;

    /// Get the order fee params.
    fn order_fee_params(&self) -> crate::Result<FeeParams<Self::Num>>;

    /// Get min collateral factor for open interest multiplier.
    fn min_collateral_factor_for_open_interest_multiplier(
        &self,
        is_long: bool,
    ) -> crate::Result<Self::Num>;

    /// Get liquidation fee params.
    fn liquidation_fee_params(&self) -> crate::Result<LiquidationFeeParams<Self::Num>>;
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
    /// # Requirements
    /// - This method must return `Ok` if
    ///   [`BaseMarket::open_interest_pool`](crate::BaseMarket::open_interest_pool) does.
    fn open_interest_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool>;

    /// Get mutable reference of open interest pool.
    /// # Requirements
    /// - This method must return `Ok` if
    ///   [`BaseMarket::open_interest_in_tokens_pool`](crate::BaseMarket::open_interest_in_tokens_pool) does.
    fn open_interest_in_tokens_pool_mut(&mut self, is_long: bool)
        -> crate::Result<&mut Self::Pool>;

    /// Get borrowing factor pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`BorrowingFeeMarket::borrowing_factor_pool`] does.
    fn borrowing_factor_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Get funding amount per size pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`PerpMarket::funding_amount_per_size_pool`] does.
    fn funding_amount_per_size_pool_mut(&mut self, is_long: bool)
        -> crate::Result<&mut Self::Pool>;

    /// Get claimable funding amount per size pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`PerpMarket::claimable_funding_amount_per_size_pool`] does.
    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool>;

    /// Get collateral sum pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if
    ///   [`BaseMarket::collateral_sum_pool`](crate::BaseMarket::collateral_sum_pool) does.
    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool>;

    /// Get total borrowing pool mutably.
    /// # Requirements
    /// - This method must return `Ok` if [`BorrowingFeeMarket::total_borrowing_pool`] does.
    fn total_borrowing_pool_mut(&mut self) -> crate::Result<&mut Self::Pool>;

    /// Insufficient funding fee payment callback.
    fn on_insufficient_funding_fee_payment(
        &mut self,
        _paid_in_collateral_amount: &Self::Num,
        _cost_amount: &Self::Num,
    ) -> crate::Result<()> {
        Ok(())
    }
}

impl<M: PerpMarket<DECIMALS>, const DECIMALS: u8> PerpMarket<DECIMALS> for &mut M {
    fn funding_factor_per_second(&self) -> &Self::Signed {
        (**self).funding_factor_per_second()
    }

    fn funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).funding_amount_per_size_pool(is_long)
    }

    fn claimable_funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        (**self).claimable_funding_amount_per_size_pool(is_long)
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

    fn min_collateral_factor_for_open_interest_multiplier(
        &self,
        is_long: bool,
    ) -> crate::Result<Self::Num> {
        (**self).min_collateral_factor_for_open_interest_multiplier(is_long)
    }

    fn liquidation_fee_params(&self) -> crate::Result<LiquidationFeeParams<Self::Num>> {
        (**self).liquidation_fee_params()
    }
}

impl<M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> PerpMarketMut<DECIMALS> for &mut M {
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

    fn on_insufficient_funding_fee_payment(
        &mut self,
        paid_in_collateral_amount: &Self::Num,
        cost_amount: &Self::Num,
    ) -> crate::Result<()> {
        (**self).on_insufficient_funding_fee_payment(paid_in_collateral_amount, cost_amount)
    }
}

/// Extension trait for [`PerpMarket`].
pub trait PerpMarketExt<const DECIMALS: u8>: PerpMarket<DECIMALS> {
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

    /// Get min collateral factor for open interest.
    fn min_collateral_factor_for_open_interest(
        &self,
        delta: &Self::Signed,
        is_long: bool,
    ) -> crate::Result<Self::Num> {
        let next_open_interest = self
            .open_interest()?
            .amount(is_long)?
            .checked_add_with_signed(delta)
            .ok_or(crate::Error::Computation(
                "calculating next OI for min collateral factor",
            ))?;
        let factor = self.min_collateral_factor_for_open_interest_multiplier(is_long)?;
        crate::utils::apply_factor(&next_open_interest, &factor).ok_or(crate::Error::Computation(
            "calculating min collateral factor for OI",
        ))
    }

    /// Caps positive position price impact in-place.
    /// If `impact` is not positive, the function does nothing.
    fn cap_positive_position_price_impact(
        &self,
        index_token_price: &Price<Self::Num>,
        size_delta_usd: &Self::Signed,
        impact: &mut Self::Signed,
    ) -> crate::Result<()> {
        use crate::{market::PositionImpactMarketExt, num::UnsignedAbs, utils};
        use num_traits::{CheckedMul, Signed};

        if impact.is_positive() {
            let impact_pool_amount = self.position_impact_pool_amount()?;
            // Cap price impact based on pool amount.
            let max_impact = impact_pool_amount
                .checked_mul(index_token_price.pick_price(false))
                .ok_or(crate::Error::Computation(
                    "overflow calculating max positive position impact based on pool amount",
                ))?
                .to_signed()?;
            if *impact > max_impact {
                *impact = max_impact;
            }

            // Cap price impact based on max factor.
            let params = self.position_params()?;
            let max_impact_factor = params.max_positive_position_impact_factor();
            let max_impact = utils::apply_factor(&size_delta_usd.unsigned_abs(), max_impact_factor)
                .ok_or(crate::Error::Computation(
                    "calculating max positive position impact based on max factor",
                ))?
                .to_signed()?;
            if *impact > max_impact {
                *impact = max_impact;
            }
        }
        Ok(())
    }

    /// Caps negative position price impact in-place.
    /// If `impact` is not negative, the function does nothing.
    ///
    /// # Returns
    ///
    /// - The capped amount of the negative `impact`.
    fn cap_negative_position_price_impact(
        &self,
        size_delta_usd: &Self::Signed,
        for_liquidations: bool,
        impact: &mut Self::Signed,
    ) -> crate::Result<Self::Num> {
        use crate::{num::UnsignedAbs, utils};
        use num_traits::{CheckedSub, Signed, Zero};

        let mut impact_diff = Zero::zero();
        if impact.is_negative() {
            let params = self.position_params()?;
            let max_impact_factor = if for_liquidations {
                params.max_position_impact_factor_for_liquidations()
            } else {
                params.max_negative_position_impact_factor()
            };
            // Although `size_delta_usd` is still used here to calculate the max impact even in the case of liquidation,
            // partial liquidation is not allowed. Therefore, `size_delta_usd == size_in_usd` always holds,
            // ensuring consistency with the Solidity version.
            let min_impact = utils::apply_factor(&size_delta_usd.unsigned_abs(), max_impact_factor)
                .ok_or(crate::Error::Computation(
                    "calculating max negative position impact based on max factor",
                ))?
                .to_opposite_signed()?;
            if *impact < min_impact {
                impact_diff = min_impact
                    .checked_sub(impact)
                    .ok_or(crate::Error::Computation(
                        "overflow calculating impact diff",
                    ))?
                    .unsigned_abs();
                *impact = min_impact;
            }
        }
        Ok(impact_diff)
    }
}

impl<M: PerpMarket<DECIMALS>, const DECIMALS: u8> PerpMarketExt<DECIMALS> for M {}

/// Extension trait for [`PerpMarketMut`].
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
