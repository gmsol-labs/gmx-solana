use std::{fmt, ops::Deref};

use num_traits::{One, Signed, Zero};

use crate::{
    action::{
        decrease_position::{DecreasePosition, DecreasePositionFlags, DecreasePositionSwapType},
        increase_position::IncreasePosition,
        swap::SwapReport,
        update_funding_state::unpack_to_funding_amount_delta,
    },
    fixed::FixedPointOps,
    market::{
        utils::MarketUtils, BaseMarketExt, BorrowingFeeMarket, BorrowingFeeMarketExt, PerpMarket,
        PerpMarketExt, PositionImpactMarket,
    },
    num::{MulDiv, Num, Unsigned, UnsignedAbs},
    params::fee::{FundingFees, PositionFees},
    price::{Price, Prices},
    Balance, BalanceExt, BaseMarket, PerpMarketMut, PnlFactorKind, Pool, PoolExt,
};

/// Read-only access to the position state.
pub trait PositionState<const DECIMALS: u8> {
    /// Unsigned number type.
    type Num: MulDiv<Signed = Self::Signed> + FixedPointOps<DECIMALS>;

    /// Signed number type.
    type Signed: UnsignedAbs<Unsigned = Self::Num> + TryFrom<Self::Num> + Num;

    /// Get the collateral amount.
    fn collateral_amount(&self) -> &Self::Num;

    /// Get a reference to the size (in USD) of the position.
    fn size_in_usd(&self) -> &Self::Num;

    /// Get a reference to the size (in tokens) of the position.
    fn size_in_tokens(&self) -> &Self::Num;

    /// Get a reference to last borrowing factor applied by the position.
    fn borrowing_factor(&self) -> &Self::Num;

    /// Get a reference to the funding fee amount per size.
    fn funding_fee_amount_per_size(&self) -> &Self::Num;

    /// Get a reference to claimable funding fee amount per size of the given collateral.
    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num;
}

/// Mutable access to the position state.
pub trait PositionStateMut<const DECIMALS: u8>: PositionState<DECIMALS> {
    /// Get a mutable reference to the collateral amount.
    fn collateral_amount_mut(&mut self) -> &mut Self::Num;

    /// Get a mutable reference to the size (in USD) of the position.
    fn size_in_usd_mut(&mut self) -> &mut Self::Num;

    /// Get a mutable reference to the size (in tokens) of the position.
    fn size_in_tokens_mut(&mut self) -> &mut Self::Num;

    /// Get a mutable reference to last borrowing factor applied by the position.
    fn borrowing_factor_mut(&mut self) -> &mut Self::Num;

    /// Get a mutable reference to the funding fee amount per size.
    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num;

    /// Get a mutable reference to claimable funding fee amount per size of the given collateral.
    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num;
}

/// Position with access to its market.
pub trait Position<const DECIMALS: u8>: PositionState<DECIMALS> {
    /// Market type.
    type Market: PerpMarket<DECIMALS, Num = Self::Num, Signed = Self::Signed>;

    /// Get a reference to the market.
    fn market(&self) -> &Self::Market;

    /// Returns whether the position is a long position.
    fn is_long(&self) -> bool;

    /// Returns whether the collateral token is the long token of the market.
    fn is_collateral_token_long(&self) -> bool;

    /// Returns whether the pnl and collateral tokens are the same.
    fn are_pnl_and_collateral_tokens_the_same(&self) -> bool;

    /// Called from `validate_position` to add supplementary checks.
    fn on_validate(&self) -> crate::Result<()>;
}

/// Position with mutable access.
pub trait PositionMut<const DECIMALS: u8>: Position<DECIMALS> + PositionStateMut<DECIMALS> {
    /// Get a mutable reference to the market.
    fn market_mut(&mut self) -> &mut Self::Market;

    /// Increased callback.
    fn on_increased(&mut self) -> crate::Result<()>;

    /// Decreased callback.
    fn on_decreased(&mut self) -> crate::Result<()>;

    /// Swapped callback.
    fn on_swapped(
        &mut self,
        ty: DecreasePositionSwapType,
        report: &SwapReport<Self::Num, <Self::Num as Unsigned>::Signed>,
    ) -> crate::Result<()>;

    /// Handle swap error.
    fn on_swap_error(
        &mut self,
        ty: DecreasePositionSwapType,
        error: crate::Error,
    ) -> crate::Result<()>;
}

impl<const DECIMALS: u8, P: PositionState<DECIMALS>> PositionState<DECIMALS> for &mut P {
    type Num = P::Num;

    type Signed = P::Signed;

    fn collateral_amount(&self) -> &Self::Num {
        (**self).collateral_amount()
    }

    fn size_in_usd(&self) -> &Self::Num {
        (**self).size_in_usd()
    }

    fn size_in_tokens(&self) -> &Self::Num {
        (**self).size_in_tokens()
    }

    fn borrowing_factor(&self) -> &Self::Num {
        (**self).borrowing_factor()
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        (**self).funding_fee_amount_per_size()
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        (**self).claimable_funding_fee_amount_per_size(is_long_collateral)
    }
}

impl<const DECIMALS: u8, P: Position<DECIMALS>> Position<DECIMALS> for &mut P {
    type Market = P::Market;

    fn market(&self) -> &Self::Market {
        (**self).market()
    }

    fn is_long(&self) -> bool {
        (**self).is_long()
    }

    fn is_collateral_token_long(&self) -> bool {
        (**self).is_collateral_token_long()
    }

    fn are_pnl_and_collateral_tokens_the_same(&self) -> bool {
        (**self).are_pnl_and_collateral_tokens_the_same()
    }

    fn on_validate(&self) -> crate::Result<()> {
        (**self).on_validate()
    }
}

impl<const DECIMALS: u8, P: PositionStateMut<DECIMALS>> PositionStateMut<DECIMALS> for &mut P {
    fn collateral_amount_mut(&mut self) -> &mut Self::Num {
        (**self).collateral_amount_mut()
    }

    fn size_in_usd_mut(&mut self) -> &mut Self::Num {
        (**self).size_in_usd_mut()
    }

    fn size_in_tokens_mut(&mut self) -> &mut Self::Num {
        (**self).size_in_tokens_mut()
    }

    fn borrowing_factor_mut(&mut self) -> &mut Self::Num {
        (**self).borrowing_factor_mut()
    }

    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num {
        (**self).funding_fee_amount_per_size_mut()
    }

    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num {
        (**self).claimable_funding_fee_amount_per_size_mut(is_long_collateral)
    }
}

impl<const DECIMALS: u8, P: PositionMut<DECIMALS>> PositionMut<DECIMALS> for &mut P {
    fn market_mut(&mut self) -> &mut Self::Market {
        (**self).market_mut()
    }

    fn on_increased(&mut self) -> crate::Result<()> {
        (**self).on_increased()
    }

    fn on_decreased(&mut self) -> crate::Result<()> {
        (**self).on_decreased()
    }

    fn on_swapped(
        &mut self,
        ty: DecreasePositionSwapType,
        report: &SwapReport<Self::Num, <Self::Num as Unsigned>::Signed>,
    ) -> crate::Result<()> {
        (**self).on_swapped(ty, report)
    }

    fn on_swap_error(
        &mut self,
        ty: DecreasePositionSwapType,
        error: crate::Error,
    ) -> crate::Result<()> {
        (**self).on_swap_error(ty, error)
    }
}

/// Extension trait for [`PositionState`].
pub trait PositionStateExt<const DECIMALS: u8>: PositionState<DECIMALS> {
    /// Return whether the position is considered to be empty **during the decrease position action**.
    fn is_empty(&self) -> bool {
        self.size_in_usd().is_zero()
            && self.size_in_tokens().is_zero()
            && self.collateral_amount().is_zero()
    }
}

impl<const DECIMALS: u8, P: PositionState<DECIMALS> + ?Sized> PositionStateExt<DECIMALS> for P {}

/// Extension trait for [`Position`] with utils.
pub trait PositionExt<const DECIMALS: u8>: Position<DECIMALS> {
    /// Check that whether the collateral will be sufficient after paying the given `realized_pnl` and applying `delta_size`.
    ///
    /// - Returns the remaining collateral value if sufficient, `None` otherwise.
    /// - Returns `Err` if failed to finish the calculation.
    fn will_collateral_be_sufficient(
        &self,
        prices: &Prices<Self::Num>,
        delta: &CollateralDelta<Self::Num>,
    ) -> crate::Result<WillCollateralBeSufficient<Self::Signed>> {
        use num_traits::{CheckedAdd, CheckedMul};

        let collateral_price = self.collateral_price(prices);

        let mut remaining_collateral_value = delta
            .next_collateral_amount
            .checked_mul(collateral_price.pick_price(false))
            .ok_or(crate::Error::Computation(
                "overflow calculating collateral value",
            ))?
            .to_signed()?;

        if delta.realized_pnl_value.is_negative() {
            remaining_collateral_value = remaining_collateral_value
                .checked_add(&delta.realized_pnl_value)
                .ok_or(crate::Error::Computation("adding realized pnl"))?;
        }

        if remaining_collateral_value.is_negative() {
            return Ok(WillCollateralBeSufficient::Insufficient(
                remaining_collateral_value,
            ));
        }

        let min_collateral_factor = self
            .market()
            .min_collateral_factor_for_open_interest(&delta.open_interest_delta, self.is_long())?
            .max(
                self.market()
                    .position_params()?
                    .min_collateral_factor()
                    .clone(),
            );

        match check_collateral(
            &delta.next_size_in_usd,
            &min_collateral_factor,
            None,
            true,
            &remaining_collateral_value,
        )? {
            CheckCollateralResult::Sufficient => Ok(WillCollateralBeSufficient::Sufficient(
                remaining_collateral_value,
            )),
            CheckCollateralResult::Negative | CheckCollateralResult::MinCollateralForLeverage => {
                Ok(WillCollateralBeSufficient::Insufficient(
                    remaining_collateral_value,
                ))
            }
            CheckCollateralResult::MinCollateral | CheckCollateralResult::Zero => unreachable!(),
        }
    }

    /// Get collateral price.
    fn collateral_price<'a>(&self, prices: &'a Prices<Self::Num>) -> &'a Price<Self::Num> {
        if self.is_collateral_token_long() {
            &prices.long_token_price
        } else {
            &prices.short_token_price
        }
    }

    /// Get collateral value.
    fn collateral_value(&self, prices: &Prices<Self::Num>) -> crate::Result<Self::Num> {
        use num_traits::CheckedMul;

        let collateral_token_price = self.collateral_price(prices).pick_price(false);

        let collateral_value = self
            .collateral_amount()
            .checked_mul(collateral_token_price)
            .ok_or(crate::Error::Computation(
                "overflow calculating collateral value",
            ))?;

        Ok(collateral_value)
    }

    /// Calculate the pnl value when decreased by the given delta size.
    ///
    /// Returns `(pnl_value, uncapped_pnl_value, size_delta_in_tokens)`
    fn pnl_value(
        &self,
        prices: &Prices<Self::Num>,
        size_delta_usd: &Self::Num,
    ) -> crate::Result<(Self::Signed, Self::Signed, Self::Num)> {
        use num_traits::{CheckedMul, CheckedSub};

        let execution_price = &prices
            .index_token_price
            .pick_price_for_pnl(self.is_long(), false);

        let position_value: Self::Signed = self
            .size_in_tokens()
            .checked_mul(execution_price)
            .ok_or(crate::Error::Computation(
                "overflow calculating position value",
            ))?
            .try_into()
            .map_err(|_| crate::Error::Convert)?;
        let size_in_usd = self
            .size_in_usd()
            .clone()
            .try_into()
            .map_err(|_| crate::Error::Convert)?;
        let mut total_pnl = if self.is_long() {
            position_value.checked_sub(&size_in_usd)
        } else {
            size_in_usd.checked_sub(&position_value)
        }
        .ok_or(crate::Error::Computation("calculating total pnl"))?;
        let uncapped_total_pnl = total_pnl.clone();

        if total_pnl.is_positive() {
            let pool_value =
                self.market()
                    .pool_value_without_pnl_for_one_side(prices, self.is_long(), false)?;
            let pool_pnl = self
                .market()
                .pnl(&prices.index_token_price, self.is_long(), true)?;
            let capped_pool_pnl = self.market().cap_pnl(
                self.is_long(),
                &pool_pnl,
                &pool_value,
                PnlFactorKind::MaxForTrader,
            )?;

            // Note: If the PnL is capped at zero, it can still pass this test.
            // See the `test_zero_max_pnl_factor_for_trader` test for more details.
            if capped_pool_pnl != pool_pnl
                && !capped_pool_pnl.is_negative()
                && pool_pnl.is_positive()
            {
                total_pnl = capped_pool_pnl
                    .unsigned_abs()
                    .checked_mul_div_with_signed_numerator(&total_pnl, &pool_pnl.unsigned_abs())
                    .ok_or(crate::Error::Computation("calculating capped total pnl"))?;
            }
        }

        let size_delta_in_tokens = if *self.size_in_usd() == *size_delta_usd {
            self.size_in_tokens().clone()
        } else if self.is_long() {
            self.size_in_tokens()
                .checked_mul_div_ceil(size_delta_usd, self.size_in_usd())
                .ok_or(crate::Error::Computation(
                    "calculating size delta in tokens for long",
                ))?
        } else {
            self.size_in_tokens()
                .checked_mul_div(size_delta_usd, self.size_in_usd())
                .ok_or(crate::Error::Computation(
                    "calculating size delta in tokens for short",
                ))?
        };

        let pnl_usd = size_delta_in_tokens
            .checked_mul_div_with_signed_numerator(&total_pnl, self.size_in_tokens())
            .ok_or(crate::Error::Computation("calculating pnl_usd"))?;

        let uncapped_pnl_usd = size_delta_in_tokens
            .checked_mul_div_with_signed_numerator(&uncapped_total_pnl, self.size_in_tokens())
            .ok_or(crate::Error::Computation("calculating uncapped_pnl_usd"))?;

        Ok((pnl_usd, uncapped_pnl_usd, size_delta_in_tokens))
    }

    /// Validate the position.
    fn validate(
        &self,
        prices: &Prices<Self::Num>,
        should_validate_min_position_size: bool,
        should_validate_min_collateral_usd: bool,
    ) -> crate::Result<()> {
        if self.size_in_usd().is_zero() || self.size_in_tokens().is_zero() {
            return Err(crate::Error::InvalidPosition(
                "size_in_usd or size_in_tokens is zero",
            ));
        }

        self.on_validate()?;

        if should_validate_min_position_size
            && self.size_in_usd() < self.market().position_params()?.min_position_size_usd()
        {
            return Err(crate::Error::InvalidPosition("size in usd too small"));
        }

        if let Some(reason) = self.check_liquidatable(prices, should_validate_min_collateral_usd)? {
            return Err(crate::Error::Liquidatable(reason));
        }

        Ok(())
    }

    /// Check if the position is liquidatable.
    ///
    /// Return [`LiquidatableReason`] if it is liquidatable, `None` otherwise.
    fn check_liquidatable(
        &self,
        prices: &Prices<Self::Num>,
        should_validate_min_collateral_usd: bool,
    ) -> crate::Result<Option<LiquidatableReason>> {
        use num_traits::{CheckedAdd, CheckedMul, CheckedSub};

        let size_in_usd = self.size_in_usd();

        let (pnl, _, _) = self.pnl_value(prices, size_in_usd)?;

        let collateral_value = self.collateral_value(prices)?;
        let collateral_price = self.collateral_price(prices);

        let size_delta_usd = size_in_usd.to_opposite_signed()?;

        let mut price_impact_value = self.position_price_impact(&size_delta_usd)?;

        let has_positive_impact = price_impact_value.is_positive();

        if price_impact_value.is_negative() {
            self.market().cap_negative_position_price_impact(
                &size_delta_usd,
                true,
                &mut price_impact_value,
            )?;
        } else {
            price_impact_value = Zero::zero();
        }

        let fees = self.position_fees(
            collateral_price,
            size_in_usd,
            has_positive_impact,
            // Should not account for liquidation fees to determine if position should be liquidated.
            false,
        )?;

        let collateral_cost_value = fees
            .total_cost_amount()?
            .checked_mul(collateral_price.pick_price(false))
            .ok_or(crate::Error::Computation(
                "overflow calculating collateral cost value",
            ))?;

        let remaining_collateral_value = collateral_value
            .to_signed()?
            .checked_add(&pnl)
            .and_then(|v| {
                v.checked_add(&price_impact_value)?
                    .checked_sub(&collateral_cost_value.to_signed().ok()?)
            })
            .ok_or(crate::Error::Computation(
                "calculating remaining collateral value",
            ))?;

        let params = self.market().position_params()?;

        match check_collateral(
            size_in_usd,
            params.min_collateral_factor(),
            should_validate_min_collateral_usd.then(|| params.min_collateral_value()),
            false,
            &remaining_collateral_value,
        )? {
            CheckCollateralResult::Sufficient => Ok(None),
            CheckCollateralResult::Zero | CheckCollateralResult::Negative => {
                Ok(Some(LiquidatableReason::NotPositive))
            }
            CheckCollateralResult::MinCollateralForLeverage => {
                Ok(Some(LiquidatableReason::MinCollateralForLeverage))
            }
            CheckCollateralResult::MinCollateral => Ok(Some(LiquidatableReason::MinCollateral)),
        }
    }

    /// Get position price impact.
    fn position_price_impact(&self, size_delta_usd: &Self::Signed) -> crate::Result<Self::Signed> {
        struct ReassignedValues<T> {
            delta_long_usd_value: T,
            delta_short_usd_value: T,
        }

        impl<T: Zero + Clone> ReassignedValues<T> {
            fn new(is_long: bool, size_delta_usd: &T) -> Self {
                if is_long {
                    Self {
                        delta_long_usd_value: size_delta_usd.clone(),
                        delta_short_usd_value: Zero::zero(),
                    }
                } else {
                    Self {
                        delta_long_usd_value: Zero::zero(),
                        delta_short_usd_value: size_delta_usd.clone(),
                    }
                }
            }
        }

        // Since the amounts of open interest are already usd amounts,
        // the price should be `one`.
        let usd_price = One::one();

        let ReassignedValues {
            delta_long_usd_value,
            delta_short_usd_value,
        } = ReassignedValues::new(self.is_long(), size_delta_usd);

        let price_impact_value = self
            .market()
            .open_interest()?
            .pool_delta_with_values(
                delta_long_usd_value,
                delta_short_usd_value,
                &usd_price,
                &usd_price,
            )?
            .price_impact(&self.market().position_impact_params()?)?;
        Ok(price_impact_value)
    }

    /// Get position price impact usd and cap the value if it is positive.
    #[inline]
    fn capped_positive_position_price_impact(
        &self,
        index_token_price: &Price<Self::Num>,
        size_delta_usd: &Self::Signed,
    ) -> crate::Result<Self::Signed> {
        let mut impact = self.position_price_impact(size_delta_usd)?;
        self.market().cap_positive_position_price_impact(
            index_token_price,
            size_delta_usd,
            &mut impact,
        )?;
        Ok(impact)
    }

    /// Get capped position price impact usd.
    ///
    /// Compare to [`PositionExt::capped_positive_position_price_impact`],
    /// this method will also cap the negative impact and return the difference before capping.
    #[inline]
    fn capped_position_price_impact(
        &self,
        index_token_price: &Price<Self::Num>,
        size_delta_usd: &Self::Signed,
    ) -> crate::Result<(Self::Signed, Self::Num)> {
        let mut impact =
            self.capped_positive_position_price_impact(index_token_price, size_delta_usd)?;
        let impact_diff =
            self.market()
                .cap_negative_position_price_impact(size_delta_usd, false, &mut impact)?;
        Ok((impact, impact_diff))
    }

    /// Get pending borrowing fee value of this position.
    fn pending_borrowing_fee_value(&self) -> crate::Result<Self::Num> {
        use crate::utils;
        use num_traits::CheckedSub;

        let latest_factor = self.market().cumulative_borrowing_factor(self.is_long())?;
        let diff_factor = latest_factor
            .checked_sub(self.borrowing_factor())
            .ok_or(crate::Error::Computation("invalid latest borrowing factor"))?;
        utils::apply_factor(self.size_in_usd(), &diff_factor)
            .ok_or(crate::Error::Computation("calculating borrowing fee value"))
    }

    /// Get pending funding fees.
    fn pending_funding_fees(&self) -> crate::Result<FundingFees<Self::Num>> {
        let adjustment = self.market().funding_amount_per_size_adjustment();
        let fees = FundingFees::builder()
            .amount(
                unpack_to_funding_amount_delta(
                    &adjustment,
                    &self.market().funding_fee_amount_per_size(
                        self.is_long(),
                        self.is_collateral_token_long(),
                    )?,
                    self.funding_fee_amount_per_size(),
                    self.size_in_usd(),
                    true,
                )
                .ok_or(crate::Error::Computation("calculating funding fee amount"))?,
            )
            .claimable_long_token_amount(
                unpack_to_funding_amount_delta(
                    &adjustment,
                    &self
                        .market()
                        .claimable_funding_fee_amount_per_size(self.is_long(), true)?,
                    self.claimable_funding_fee_amount_per_size(true),
                    self.size_in_usd(),
                    false,
                )
                .ok_or(crate::Error::Computation(
                    "calculating claimable long token funding fee amount",
                ))?,
            )
            .claimable_short_token_amount(
                unpack_to_funding_amount_delta(
                    &adjustment,
                    &self
                        .market()
                        .claimable_funding_fee_amount_per_size(self.is_long(), false)?,
                    self.claimable_funding_fee_amount_per_size(false),
                    self.size_in_usd(),
                    false,
                )
                .ok_or(crate::Error::Computation(
                    "calculating claimable short token funding fee amount",
                ))?,
            )
            .build();
        Ok(fees)
    }

    /// Calculates the [`PositionFees`] generated by changing the position size by the specified `size_delta_usd`.
    fn position_fees(
        &self,
        collateral_token_price: &Price<Self::Num>,
        size_delta_usd: &Self::Num,
        is_positive_impact: bool,
        is_liquidation: bool,
    ) -> crate::Result<PositionFees<Self::Num>> {
        debug_assert!(!collateral_token_price.has_zero(), "must be non-zero");

        let liquidation_fees = is_liquidation
            .then(|| {
                // Although `size_delta_usd` is used here to calculate liquidation fee, partial liquidation is not allowed.
                // Therefore, `size_delta_usd == size_in_usd` always holds, ensuring consistency with the Solidity version.
                self.market()
                    .liquidation_fee_params()?
                    .fee(size_delta_usd, collateral_token_price)
            })
            .transpose()?;

        let fees = self
            .market()
            .order_fee_params()?
            .base_position_fees(collateral_token_price, size_delta_usd, is_positive_impact)?
            .set_borrowing_fees(
                self.market().borrowing_fee_params()?.receiver_factor(),
                collateral_token_price,
                self.pending_borrowing_fee_value()?,
            )?
            .set_funding_fees(self.pending_funding_fees()?)
            .set_liquidation_fees(liquidation_fees);
        Ok(fees)
    }
}

impl<const DECIMALS: u8, P: Position<DECIMALS>> PositionExt<DECIMALS> for P {}

/// Extension trait for [`PositionMut`] with utils.
pub trait PositionMutExt<const DECIMALS: u8>: PositionMut<DECIMALS>
where
    Self::Market: PerpMarketMut<DECIMALS, Num = Self::Num, Signed = Self::Signed>,
{
    /// Create an action to increase the position.
    fn increase(
        &mut self,
        prices: Prices<Self::Num>,
        collateral_increment_amount: Self::Num,
        size_delta_usd: Self::Num,
        acceptable_price: Option<Self::Num>,
    ) -> crate::Result<IncreasePosition<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        IncreasePosition::try_new(
            self,
            prices,
            collateral_increment_amount,
            size_delta_usd,
            acceptable_price,
        )
    }

    /// Create an action to decrease the position.
    fn decrease(
        &mut self,
        prices: Prices<Self::Num>,
        size_delta_usd: Self::Num,
        acceptable_price: Option<Self::Num>,
        collateral_withdrawal_amount: Self::Num,
        flags: DecreasePositionFlags,
    ) -> crate::Result<DecreasePosition<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        DecreasePosition::try_new(
            self,
            prices,
            size_delta_usd,
            acceptable_price,
            collateral_withdrawal_amount,
            flags,
        )
    }

    /// Update global open interest.
    fn update_open_interest(
        &mut self,
        size_delta_usd: &Self::Signed,
        size_delta_in_tokens: &Self::Signed,
    ) -> crate::Result<()> {
        use num_traits::CheckedAdd;

        if size_delta_usd.is_zero() {
            return Ok(());
        }
        let is_long_collateral = self.is_collateral_token_long();
        let is_long = self.is_long();
        let max_open_interest = self.market().max_open_interest(is_long)?;

        let open_interest = self.market_mut().open_interest_pool_mut(is_long)?;
        if is_long_collateral {
            open_interest.apply_delta_to_long_amount(size_delta_usd)?;
        } else {
            open_interest.apply_delta_to_short_amount(size_delta_usd)?;
        }

        if size_delta_usd.is_positive() {
            let is_exceeded = open_interest
                .long_amount()?
                .checked_add(&open_interest.short_amount()?)
                .map(|total| total > max_open_interest)
                .unwrap_or(true);

            if is_exceeded {
                return Err(crate::Error::MaxOpenInterestExceeded);
            }
        }

        let open_interest_in_tokens = self
            .market_mut()
            .open_interest_in_tokens_pool_mut(is_long)?;
        if is_long_collateral {
            open_interest_in_tokens.apply_delta_to_long_amount(size_delta_in_tokens)?;
        } else {
            open_interest_in_tokens.apply_delta_to_short_amount(size_delta_in_tokens)?;
        }

        Ok(())
    }

    /// Update total borrowing.
    fn update_total_borrowing(
        &mut self,
        next_size_in_usd: &Self::Num,
        next_borrowing_factor: &Self::Num,
    ) -> crate::Result<()> {
        let is_long = self.is_long();
        let previous = crate::utils::apply_factor(self.size_in_usd(), self.borrowing_factor())
            .ok_or(crate::Error::Computation("calculating previous borrowing"))?;

        let total_borrowing = self.market_mut().total_borrowing_pool_mut()?;

        let delta = {
            let next = crate::utils::apply_factor(next_size_in_usd, next_borrowing_factor)
                .ok_or(crate::Error::Computation("calculating next borrowing"))?;
            next.checked_signed_sub(previous)?
        };

        total_borrowing.apply_delta_amount(is_long, &delta)?;

        Ok(())
    }
}

impl<const DECIMALS: u8, P: PositionMut<DECIMALS>> PositionMutExt<DECIMALS> for P where
    P::Market: PerpMarketMut<DECIMALS, Num = Self::Num, Signed = Self::Signed>
{
}

/// Collateral Delta Values.
pub struct CollateralDelta<T: Unsigned> {
    next_size_in_usd: T,
    next_collateral_amount: T,
    realized_pnl_value: T::Signed,
    open_interest_delta: T::Signed,
}

impl<T: Unsigned> CollateralDelta<T> {
    /// Create a new collateral delta.
    pub fn new(
        next_size_in_usd: T,
        next_collateral_amount: T,
        realized_pnl_value: T::Signed,
        open_interest_delta: T::Signed,
    ) -> Self {
        Self {
            next_size_in_usd,
            next_collateral_amount,
            realized_pnl_value,
            open_interest_delta,
        }
    }
}

/// Will collateral be sufficient.
#[derive(Clone, Copy)]
pub enum WillCollateralBeSufficient<T> {
    /// Will be sufficient.
    Sufficient(T),
    /// Won't be sufficient.
    Insufficient(T),
}

impl<T> WillCollateralBeSufficient<T> {
    /// Returns whether it is sufficient.
    pub fn is_sufficient(&self) -> bool {
        matches!(self, Self::Sufficient(_))
    }
}

impl<T> Deref for WillCollateralBeSufficient<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Sufficient(v) => v,
            Self::Insufficient(v) => v,
        }
    }
}

/// Liquidatable reason.
#[derive(Debug, Clone, Copy)]
pub enum LiquidatableReason {
    /// Min collateral.
    MinCollateral,
    /// Remaining collateral not positive.
    NotPositive,
    /// Min collateral for leverage.
    MinCollateralForLeverage,
}

impl fmt::Display for LiquidatableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MinCollateral => write!(f, "min collateral"),
            Self::NotPositive => write!(f, "<= 0"),
            Self::MinCollateralForLeverage => write!(f, "min collateral for leverage"),
        }
    }
}

enum CheckCollateralResult {
    Sufficient,
    Zero,
    Negative,
    MinCollateralForLeverage,
    MinCollateral,
}

fn check_collateral<T, const DECIMALS: u8>(
    size_in_usd: &T,
    min_collateral_factor: &T,
    min_collateral_value: Option<&T>,
    allow_zero_collateral: bool,
    collateral_value: &T::Signed,
) -> crate::Result<CheckCollateralResult>
where
    T: FixedPointOps<DECIMALS>,
{
    if collateral_value.is_negative() {
        if min_collateral_value.is_some() {
            // Keep the behavior consistent with the Solidity version.
            Ok(CheckCollateralResult::MinCollateral)
        } else {
            Ok(CheckCollateralResult::Negative)
        }
    } else {
        let collateral_value = collateral_value.unsigned_abs();

        if let Some(min_collateral_value) = min_collateral_value {
            if collateral_value < *min_collateral_value {
                return Ok(CheckCollateralResult::MinCollateral);
            }
        }

        if !allow_zero_collateral && collateral_value.is_zero() {
            return Ok(CheckCollateralResult::Zero);
        }

        let min_collateral_usd_for_leverage =
            crate::utils::apply_factor(size_in_usd, min_collateral_factor).ok_or(
                crate::Error::Computation("calculating min collateral usd for leverage"),
            )?;

        if collateral_value < min_collateral_usd_for_leverage {
            return Ok(CheckCollateralResult::MinCollateralForLeverage);
        }

        Ok(CheckCollateralResult::Sufficient)
    }
}

/// Insolvent Close Step.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "anchor-lang",
    derive(
        anchor_lang::AnchorDeserialize,
        anchor_lang::AnchorSerialize,
        anchor_lang::InitSpace
    )
)]
#[non_exhaustive]
pub enum InsolventCloseStep {
    /// PnL.
    Pnl,
    /// Fees.
    Fees,
    /// Funding fees.
    Funding,
    /// Price impact.
    Impact,
    /// Price impact diff.
    Diff,
}
