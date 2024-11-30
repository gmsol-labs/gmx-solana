use num_traits::{CheckedAdd, CheckedNeg, Signed, Zero};
use std::fmt;

use crate::{
    market::{
        BaseMarketExt, BaseMarketMutExt, PerpMarketExt, PerpMarketMutExt,
        PositionImpactMarketMutExt,
    },
    num::Unsigned,
    params::fee::PositionFees,
    position::{CollateralDelta, Position, PositionExt},
    price::{Price, Prices},
    BorrowingFeeMarketExt, PerpMarketMut, PoolExt, PositionMut, PositionMutExt,
};

use super::{
    update_borrowing_state::UpdateBorrowingReport, update_funding_state::UpdateFundingReport,
};

/// Increase the position.
#[must_use]
pub struct IncreasePosition<P: Position<DECIMALS>, const DECIMALS: u8> {
    position: P,
    params: IncreasePositionParams<P::Num>,
}

/// Increase Position Params.
#[derive(Debug, Clone, Copy)]
pub struct IncreasePositionParams<T> {
    collateral_increment_amount: T,
    size_delta_usd: T,
    acceptable_price: Option<T>,
    prices: Prices<T>,
}

impl<T> IncreasePositionParams<T> {
    /// Get prices.
    pub fn prices(&self) -> &Prices<T> {
        &self.prices
    }
}

/// Report of the execution of position increasing.
#[must_use]
pub struct IncreasePositionReport<T: Unsigned> {
    params: IncreasePositionParams<T>,
    execution: ExecutionParams<T>,
    collateral_delta_amount: T::Signed,
    fees: PositionFees<T>,
    borrowing: UpdateBorrowingReport<T>,
    funding: UpdateFundingReport<T>,
    /// Output amounts that must be processed.
    claimable_funding_long_token_amount: T,
    claimable_funding_short_token_amount: T,
}

impl<T: Unsigned + fmt::Debug> fmt::Debug for IncreasePositionReport<T>
where
    T::Signed: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncreasePositionReport")
            .field("params", &self.params)
            .field("execution", &self.execution)
            .field("collateral_delta_amount", &self.collateral_delta_amount)
            .field("fees", &self.fees)
            .field("borrowing", &self.borrowing)
            .field("funding", &self.funding)
            .field(
                "claimable_funding_long_token_amount",
                &self.claimable_funding_long_token_amount,
            )
            .field(
                "claimable_funding_short_token_amount",
                &self.claimable_funding_short_token_amount,
            )
            .finish()
    }
}

impl<T: Unsigned + Clone> IncreasePositionReport<T> {
    fn new(
        params: IncreasePositionParams<T>,
        execution: ExecutionParams<T>,
        collateral_delta_amount: T::Signed,
        fees: PositionFees<T>,
        borrowing: UpdateBorrowingReport<T>,
        funding: UpdateFundingReport<T>,
    ) -> Self {
        let claimable_funding_long_token_amount =
            fees.funding_fees().claimable_long_token_amount().clone();
        let claimable_funding_short_token_amount =
            fees.funding_fees().claimable_short_token_amount().clone();
        Self {
            params,
            execution,
            collateral_delta_amount,
            fees,
            borrowing,
            funding,
            claimable_funding_long_token_amount,
            claimable_funding_short_token_amount,
        }
    }

    /// Get claimable funding amounts, returns `(long_amount, short_amount)`.
    ///
    /// ## Must Use
    /// These amounts are expected to be used by the caller.
    pub fn claimable_funding_amounts(&self) -> (&T, &T) {
        (
            &self.claimable_funding_long_token_amount,
            &self.claimable_funding_short_token_amount,
        )
    }

    /// Get params.
    pub fn params(&self) -> &IncreasePositionParams<T> {
        &self.params
    }

    /// Get execution params.
    pub fn execution(&self) -> &ExecutionParams<T> {
        &self.execution
    }

    /// Get collateral delta amount.
    pub fn collateral_delta_amount(&self) -> &T::Signed {
        &self.collateral_delta_amount
    }

    /// Get position fees.
    pub fn fees(&self) -> &PositionFees<T> {
        &self.fees
    }

    /// Get borrowing report.
    pub fn borrowing(&self) -> &UpdateBorrowingReport<T> {
        &self.borrowing
    }

    /// Get funding report.
    pub fn funding(&self) -> &UpdateFundingReport<T> {
        &self.funding
    }
}

/// Execution Params for increasing position.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionParams<T: Unsigned> {
    price_impact_value: T::Signed,
    price_impact_amount: T::Signed,
    size_delta_in_tokens: T,
    execution_price: T,
}

impl<T: Unsigned> ExecutionParams<T> {
    /// Get execution price.
    pub fn execution_price(&self) -> &T {
        &self.execution_price
    }

    /// Get price impact amount.
    pub fn price_impact_amount(&self) -> &T::Signed {
        &self.price_impact_amount
    }

    /// Get price impact value.
    pub fn price_impact_value(&self) -> &T::Signed {
        &self.price_impact_value
    }
}

impl<const DECIMALS: u8, P: PositionMut<DECIMALS>> IncreasePosition<P, DECIMALS>
where
    P::Market: PerpMarketMut<DECIMALS, Num = P::Num, Signed = P::Signed>,
{
    /// Create a new action to increase the given position.
    pub fn try_new(
        position: P,
        prices: Prices<P::Num>,
        collateral_increment_amount: P::Num,
        size_delta_usd: P::Num,
        acceptable_price: Option<P::Num>,
    ) -> crate::Result<Self> {
        if !prices.is_valid() {
            return Err(crate::Error::InvalidArgument("invalid prices"));
        }
        Ok(Self {
            position,
            params: IncreasePositionParams {
                collateral_increment_amount,
                size_delta_usd,
                acceptable_price,
                prices,
            },
        })
    }

    /// Execute.
    pub fn execute(mut self) -> crate::Result<IncreasePositionReport<P::Num>> {
        let borrowing = self
            .position
            .market_mut()
            .update_borrowing(&self.params.prices)?
            .execute()?;
        let funding = self
            .position
            .market_mut()
            .update_funding(&self.params.prices)?
            .execute()?;

        self.initialize_position_if_empty()?;

        let execution = self.get_execution_params()?;

        let (collateral_delta_amount, fees) =
            self.process_collateral(&execution.price_impact_value)?;

        let is_collateral_delta_positive = collateral_delta_amount.is_positive();
        *self.position.collateral_amount_mut() = self
            .position
            .collateral_amount_mut()
            .checked_add_with_signed(&collateral_delta_amount)
            .ok_or_else(|| {
                if is_collateral_delta_positive {
                    crate::Error::Computation("collateral amount overflow")
                } else {
                    crate::Error::InvalidArgument("insufficient collateral amount")
                }
            })?;

        self.position
            .market_mut()
            .apply_delta_to_position_impact_pool(
                &execution
                    .price_impact_amount()
                    .checked_neg()
                    .ok_or(crate::Error::Computation(
                        "calculating position impact pool delta amount",
                    ))?,
            )?;

        let is_long = self.position.is_long();
        let next_position_size_in_usd = self
            .position
            .size_in_usd_mut()
            .checked_add(&self.params.size_delta_usd)
            .ok_or(crate::Error::Computation("size in usd overflow"))?;
        let next_position_borrowing_factor = self
            .position
            .market()
            .cumulative_borrowing_factor(is_long)?;

        // Update total borrowing before updating position size.
        self.position
            .update_total_borrowing(&next_position_size_in_usd, &next_position_borrowing_factor)?;

        // Update sizes.
        *self.position.size_in_usd_mut() = next_position_size_in_usd;
        *self.position.size_in_tokens_mut() = self
            .position
            .size_in_tokens_mut()
            .checked_add(&execution.size_delta_in_tokens)
            .ok_or(crate::Error::Computation("size in tokens overflow"))?;

        // Update funding fees state.
        *self.position.funding_fee_amount_per_size_mut() = self
            .position
            .market()
            .funding_fee_amount_per_size(is_long, self.position.is_collateral_token_long())?;
        for is_long_collateral in [true, false] {
            *self
                .position
                .claimable_funding_fee_amount_per_size_mut(is_long_collateral) = self
                .position
                .market()
                .claimable_funding_fee_amount_per_size(is_long, is_long_collateral)?;
        }

        // Update borrowing fee state.
        *self.position.borrowing_factor_mut() = next_position_borrowing_factor;

        self.position.update_open_interest(
            &self.params.size_delta_usd.to_signed()?,
            &execution.size_delta_in_tokens.to_signed()?,
        )?;

        if !self.params.size_delta_usd.is_zero() {
            let market = self.position.market();
            market.validate_reserve(&self.params.prices, self.position.is_long())?;
            market.validate_open_interest_reserve(&self.params.prices, self.position.is_long())?;

            let delta = CollateralDelta::new(
                self.position.size_in_usd().clone(),
                self.position.collateral_amount().clone(),
                Zero::zero(),
                Zero::zero(),
            );
            let will_collateral_be_sufficient = self
                .position
                .will_collateral_be_sufficient(&self.params.prices, &delta)?;

            if !will_collateral_be_sufficient.is_sufficient() {
                return Err(crate::Error::InvalidArgument("insufficient collateral usd"));
            }
        }

        self.position.validate(&self.params.prices, true, true)?;

        self.position.on_increased()?;

        Ok(IncreasePositionReport::new(
            self.params,
            execution,
            collateral_delta_amount,
            fees,
            borrowing,
            funding,
        ))
    }

    fn initialize_position_if_empty(&mut self) -> crate::Result<()> {
        if self.position.size_in_usd().is_zero() {
            // Ensure that the size in tokens is initialized to zero.
            *self.position.size_in_tokens_mut() = P::Num::zero();
            let funding_fee_amount_per_size = self.position.market().funding_fee_amount_per_size(
                self.position.is_long(),
                self.position.is_collateral_token_long(),
            )?;
            *self.position.funding_fee_amount_per_size_mut() = funding_fee_amount_per_size;
            for is_long_collateral in [true, false] {
                let claimable_funding_fee_amount_per_size = self
                    .position
                    .market()
                    .claimable_funding_fee_amount_per_size(
                        self.position.is_long(),
                        is_long_collateral,
                    )?;
                *self
                    .position
                    .claimable_funding_fee_amount_per_size_mut(is_long_collateral) =
                    claimable_funding_fee_amount_per_size;
            }
        }
        Ok(())
    }

    fn get_execution_params(&self) -> crate::Result<ExecutionParams<P::Num>> {
        let index_token_price = &self.params.prices.index_token_price;
        if self.params.size_delta_usd.is_zero() {
            return Ok(ExecutionParams {
                price_impact_value: Zero::zero(),
                price_impact_amount: Zero::zero(),
                size_delta_in_tokens: Zero::zero(),
                execution_price: index_token_price
                    .pick_price(self.position.is_long())
                    .clone(),
            });
        }

        let price_impact_value = self.position.capped_positive_position_price_impact(
            index_token_price,
            &self.params.size_delta_usd.to_signed()?,
        )?;

        let price_impact_amount = if price_impact_value.is_positive() {
            let price: P::Signed = self
                .params
                .prices
                .index_token_price
                .pick_price(true)
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?;
            debug_assert!(
                !price.is_zero(),
                "price must have been checked to be non-zero"
            );
            price_impact_value.clone() / price
        } else {
            self.params
                .prices
                .index_token_price
                .pick_price(false)
                .as_divisor_to_round_up_magnitude_div(&price_impact_value)
                .ok_or(crate::Error::Computation("calculating price impact amount"))?
        };

        // Base size delta in tokens.
        let mut size_delta_in_tokens = if self.position.is_long() {
            let price = self.params.prices.index_token_price.pick_price(true);
            debug_assert!(
                !price.is_zero(),
                "price must have been checked to be non-zero"
            );
            self.params.size_delta_usd.clone() / price.clone()
        } else {
            let price = self.params.prices.index_token_price.pick_price(false);
            self.params
                .size_delta_usd
                .checked_round_up_div(price)
                .ok_or(crate::Error::Computation(
                    "calculating size delta in tokens",
                ))?
        };

        // Apply price impact.
        size_delta_in_tokens = if self.position.is_long() {
            size_delta_in_tokens.checked_add_with_signed(&price_impact_amount)
        } else {
            size_delta_in_tokens.checked_sub_with_signed(&price_impact_amount)
        }
        .ok_or(crate::Error::Computation(
            "price impact larger than order size",
        ))?;

        let execution_price = get_execution_price_for_increase(
            &self.params.size_delta_usd,
            &size_delta_in_tokens,
            self.params.acceptable_price.as_ref(),
            self.position.is_long(),
        )?;

        Ok(ExecutionParams {
            price_impact_value,
            price_impact_amount,
            size_delta_in_tokens,
            execution_price,
        })
    }

    #[inline]
    fn collateral_price(&self) -> &Price<P::Num> {
        self.position.collateral_price(&self.params.prices)
    }

    fn process_collateral(
        &mut self,
        price_impact_value: &P::Signed,
    ) -> crate::Result<(P::Signed, PositionFees<P::Num>)> {
        use num_traits::CheckedSub;

        let mut collateral_delta_amount = self.params.collateral_increment_amount.to_signed()?;

        let fees = self.position.position_fees(
            self.collateral_price(),
            &self.params.size_delta_usd,
            price_impact_value.is_positive(),
        )?;

        collateral_delta_amount = collateral_delta_amount
            .checked_sub(&fees.total_cost_amount()?.to_signed()?)
            .ok_or(crate::Error::Computation(
                "applying fees to collateral amount",
            ))?;

        let is_collateral_token_long = self.position.is_collateral_token_long();

        self.position
            .market_mut()
            .apply_delta_to_claimable_fee_pool(
                is_collateral_token_long,
                &fees.for_receiver()?.to_signed()?,
            )?;

        self.position
            .market_mut()
            .apply_delta(is_collateral_token_long, &fees.for_pool()?.to_signed()?)?;

        let is_long = self.position.is_long();
        self.position
            .market_mut()
            .collateral_sum_pool_mut(is_long)?
            .apply_delta_amount(is_collateral_token_long, &collateral_delta_amount)?;

        Ok((collateral_delta_amount, fees))
    }
}

fn get_execution_price_for_increase<T>(
    size_delta_usd: &T,
    size_delta_in_tokens: &T,
    acceptable_price: Option<&T>,
    is_long: bool,
) -> crate::Result<T>
where
    T: num_traits::Num + Ord + Clone,
{
    if size_delta_usd.is_zero() {
        return Err(crate::Error::Computation("empty size delta in tokens"));
    }

    let execution_price = size_delta_usd.clone() / size_delta_in_tokens.clone();

    let Some(acceptable_price) = acceptable_price else {
        return Ok(execution_price);
    };

    if (is_long && execution_price <= *acceptable_price)
        || (!is_long && execution_price >= *acceptable_price)
    {
        Ok(execution_price)
    } else {
        Err(crate::Error::InvalidArgument(
            "order not fulfillable at acceptable price",
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        market::LiquidityMarketMutExt,
        test::{TestMarket, TestPosition},
    };

    use super::*;

    #[test]
    fn basic() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");
        let mut position = TestPosition::long(true);
        let report = position
            .ops(&mut market)
            .increase(
                Prices::new_for_test(123, 123, 1),
                100_000_000,
                8_000_000_000,
                None,
            )?
            .execute()?;
        println!("{report:#?}");
        println!("{position:#?}");
        Ok(())
    }
}
