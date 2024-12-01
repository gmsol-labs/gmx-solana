use num_traits::{CheckedAdd, CheckedSub, Zero};

use crate::{
    market::{PerpMarket, PerpMarketExt, SwapMarketMutExt},
    num::{MulDiv, Unsigned},
    params::fee::PositionFees,
    position::{
        CollateralDelta, Position, PositionExt, PositionMut, PositionMutExt, PositionStateExt,
        WillCollateralBeSufficient,
    },
    price::{Price, Prices},
    BorrowingFeeMarketExt, PerpMarketMut, PerpMarketMutExt, PoolExt,
};

use self::collateral_processor::{CollateralProcessor, ProcessResult};

mod claimable;
mod collateral_processor;
mod report;
mod utils;

pub use self::{
    claimable::ClaimableCollateral,
    report::{DecreasePositionReport, OutputAmounts, Pnl},
};

/// Decrease the position.
#[must_use]
pub struct DecreasePosition<P: Position<DECIMALS>, const DECIMALS: u8> {
    position: P,
    params: DecreasePositionParams<P::Num>,
    withdrawable_collateral_amount: P::Num,
    size_delta_usd: P::Num,
}

/// Swap Type for the decrese position action.
#[derive(Debug, Clone, Copy, Default)]
pub enum DecreasePositionSwapType {
    /// No swap.
    #[default]
    NoSwap,
    /// Swap PnL token to collateral token.
    PnlTokenToCollateralToken,
    /// Swap collateral token to PnL token.
    CollateralToPnlToken,
}

/// Decrease Position Params.
#[derive(Debug, Clone, Copy)]
pub struct DecreasePositionParams<T> {
    prices: Prices<T>,
    initial_size_delta_usd: T,
    acceptable_price: Option<T>,
    initial_collateral_withdrawal_amount: T,
    is_insolvent_close_allowed: bool,
    is_liquidation_order: bool,
    swap: DecreasePositionSwapType,
}

impl<T> DecreasePositionParams<T> {
    /// Get initial collateral withdrawal amount.
    pub fn initial_collateral_withdrawal_amount(&self) -> &T {
        &self.initial_collateral_withdrawal_amount
    }

    /// Get inital size delta usd.
    pub fn initial_size_delta_usd(&self) -> &T {
        &self.initial_size_delta_usd
    }

    /// Get prices.
    pub fn prices(&self) -> &Prices<T> {
        &self.prices
    }
}

struct ProcessCollateralResult<T: Unsigned> {
    price_impact_value: T::Signed,
    price_impact_diff: T,
    execution_price: T,
    size_delta_in_tokens: T,
    is_output_token_long: bool,
    is_secondary_output_token_long: bool,
    collateral: ProcessResult<T>,
    fees: PositionFees<T>,
    pnl: Pnl<T::Signed>,
}

impl<const DECIMALS: u8, P: PositionMut<DECIMALS>> DecreasePosition<P, DECIMALS>
where
    P::Market: PerpMarketMut<DECIMALS, Num = P::Num, Signed = P::Signed>,
{
    /// Create a new action to decrease the given position.
    pub fn try_new(
        position: P,
        prices: Prices<P::Num>,
        size_delta_usd: P::Num,
        acceptable_price: Option<P::Num>,
        collateral_withdrawal_amount: P::Num,
        is_insolvent_close_allowed: bool,
        is_liquidation_order: bool,
    ) -> crate::Result<Self> {
        if !prices.is_valid() {
            return Err(crate::Error::InvalidArgument("invalid prices"));
        }
        if position.is_empty() {
            return Err(crate::Error::InvalidPosition("empty position"));
        }
        let size_delta_usd = size_delta_usd.min(position.size_in_usd().clone());
        Ok(Self {
            params: DecreasePositionParams {
                prices,
                initial_size_delta_usd: size_delta_usd.clone(),
                acceptable_price,
                initial_collateral_withdrawal_amount: collateral_withdrawal_amount.clone(),
                is_insolvent_close_allowed: is_insolvent_close_allowed
                    && (size_delta_usd == *position.size_in_usd())
                    && is_liquidation_order,
                is_liquidation_order,
                swap: DecreasePositionSwapType::NoSwap,
            },
            withdrawable_collateral_amount: collateral_withdrawal_amount
                .min(position.collateral_amount().clone()),
            size_delta_usd,
            position,
        })
    }

    /// Set the swap type.
    pub fn set_swap(mut self, kind: DecreasePositionSwapType) -> Self {
        self.params.swap = kind;
        self
    }

    /// Execute.
    pub fn execute(mut self) -> crate::Result<Box<DecreasePositionReport<P::Num>>> {
        debug_assert!(
            self.size_delta_usd <= *self.position.size_in_usd_mut(),
            "must have been checked or capped by the position size"
        );
        debug_assert!(
            self.withdrawable_collateral_amount <= *self.position.collateral_amount_mut(),
            "must have been capped by the position collateral amount"
        );

        self.check_partial_close()?;
        self.check_close()?;

        if !matches!(self.params.swap, DecreasePositionSwapType::NoSwap)
            && self.position.are_pnl_and_collateral_tokens_the_same()
        {
            self.params.swap = DecreasePositionSwapType::NoSwap;
        }

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

        self.check_liquiation()?;

        let initial_collateral_amount = self.position.collateral_amount_mut().clone();

        let mut execution = self.process_collateral()?;

        let should_remove;
        {
            let is_long = self.position.is_long();
            let is_collateral_long = self.position.is_collateral_token_long();

            let next_position_size_in_usd = self
                .position
                .size_in_usd_mut()
                .checked_sub(&self.size_delta_usd)
                .ok_or(crate::Error::Computation(
                    "calculating next position size in usd",
                ))?;
            let next_position_borrowing_factor = self
                .position
                .market()
                .cumulative_borrowing_factor(is_long)?;

            // Update total borrowing before updating position size.
            self.position.update_total_borrowing(
                &next_position_size_in_usd,
                &next_position_borrowing_factor,
            )?;

            let next_position_size_in_tokens = self
                .position
                .size_in_tokens_mut()
                .checked_sub(&execution.size_delta_in_tokens)
                .ok_or(crate::Error::Computation("calculating next size in tokens"))?;
            let next_position_collateral_amount =
                execution.collateral.remaining_collateral_amount.clone();

            should_remove =
                next_position_size_in_usd.is_zero() || next_position_size_in_tokens.is_zero();

            if should_remove {
                *self.position.size_in_usd_mut() = Zero::zero();
                *self.position.size_in_tokens_mut() = Zero::zero();
                *self.position.collateral_amount_mut() = Zero::zero();
                execution.collateral.output_amount = execution
                    .collateral
                    .output_amount
                    .checked_add(&next_position_collateral_amount)
                    .ok_or(crate::Error::Computation("calculating output amount"))?;
            } else {
                *self.position.size_in_usd_mut() = next_position_size_in_usd;
                *self.position.size_in_tokens_mut() = next_position_size_in_tokens;
                *self.position.collateral_amount_mut() = next_position_collateral_amount;
            };

            // Update collateral sum.
            {
                let collateral_delta_amount = initial_collateral_amount
                    .checked_sub(self.position.collateral_amount_mut())
                    .ok_or(crate::Error::Computation("collateral amount increased"))?;

                self.position
                    .market_mut()
                    .collateral_sum_pool_mut(is_long)?
                    .apply_delta_amount(
                        is_collateral_long,
                        &collateral_delta_amount.to_opposite_signed()?,
                    )?;
            }

            // The state of the position must be up-to-date, even if it is going to be removed.
            *self.position.borrowing_factor_mut() = next_position_borrowing_factor;
            *self.position.funding_fee_amount_per_size_mut() = self
                .position
                .market()
                .funding_fee_amount_per_size(is_long, is_collateral_long)?;
            for is_long_collateral in [true, false] {
                *self
                    .position
                    .claimable_funding_fee_amount_per_size_mut(is_long_collateral) = self
                    .position
                    .market()
                    .claimable_funding_fee_amount_per_size(is_long, is_long_collateral)?;
            }
        }

        // Update open interest.
        self.position.update_open_interest(
            &self.size_delta_usd.to_opposite_signed()?,
            &execution.size_delta_in_tokens.to_opposite_signed()?,
        )?;

        if !should_remove {
            self.position.validate(&self.params.prices, false, false)?;
        }

        self.position.on_decreased()?;

        let mut report = Box::new(DecreasePositionReport::new(
            should_remove,
            // self.params,
            execution,
            self.withdrawable_collateral_amount,
            self.size_delta_usd,
            borrowing,
            funding,
        ));

        let are_pnl_and_collateral_tokens_the_same =
            self.position.are_pnl_and_collateral_tokens_the_same();
        Self::swap_output_tokens_if_needed(
            self.position.market_mut(),
            &mut report,
            self.params.prices(),
            are_pnl_and_collateral_tokens_the_same,
        )?;

        Ok(report)
    }

    /// Do a check when the position will be partially decreased.
    fn check_partial_close(&mut self) -> crate::Result<()> {
        use num_traits::CheckedMul;

        if self.is_partial_close() {
            let (estimated_pnl, _, _) = self
                .position
                .pnl_value(&self.params.prices, self.position.size_in_usd())?;
            let estimated_realized_pnl = self
                .size_delta_usd
                .checked_mul_div_with_signed_numerator(&estimated_pnl, self.position.size_in_usd())
                .ok_or(crate::Error::Computation("estimating realized pnl"))?;
            let estimated_remaining_pnl = estimated_pnl
                .checked_sub(&estimated_realized_pnl)
                .ok_or(crate::Error::Computation("estimating remaining pnl"))?;

            let delta = CollateralDelta::new(
                self.position
                    .size_in_usd()
                    .checked_sub(&self.size_delta_usd)
                    .expect("should have been capped"),
                self.position
                    .collateral_amount()
                    .checked_sub(&self.withdrawable_collateral_amount)
                    .expect("should have been capped"),
                estimated_realized_pnl,
                self.size_delta_usd.to_opposite_signed()?,
            );

            let mut will_be_sufficient = self
                .position
                .will_collateral_be_sufficient(&self.params.prices, &delta)?;

            if let WillCollateralBeSufficient::Insufficient(remaining_collateral_value) =
                &mut will_be_sufficient
            {
                if self.size_delta_usd.is_zero() {
                    return Err(crate::Error::InvalidArgument(
                        "unable to withdraw collateral: insufficient collateral",
                    ));
                }

                let collateral_token_price = if self.position.is_collateral_token_long() {
                    &self.params.prices.long_token_price
                } else {
                    &self.params.prices.short_token_price
                };
                // Add back to the estimated remaining collateral value && set withdrawable collateral amount to zero.
                let add_back = self
                    .withdrawable_collateral_amount
                    .checked_mul(collateral_token_price.pick_price(false))
                    .ok_or(crate::Error::Computation("overflow calculating add back"))?
                    .to_signed()?;
                *remaining_collateral_value = remaining_collateral_value
                    .checked_add(&add_back)
                    .ok_or(crate::Error::Computation("adding back"))?;
                self.withdrawable_collateral_amount = Zero::zero();
            }

            // Close all if collateral or position size too small.

            let params = self.position.market().position_params()?;

            let remaining_value = will_be_sufficient
                .checked_add(&estimated_remaining_pnl)
                .ok_or(crate::Error::Computation("calculating remaining value"))?;
            if remaining_value < params.min_collateral_value().to_signed()? {
                self.size_delta_usd = self.position.size_in_usd().clone();
            }

            if *self.position.size_in_usd() > self.size_delta_usd
                && self
                    .position
                    .size_in_usd()
                    .checked_sub(&self.size_delta_usd)
                    .expect("must success")
                    < *params.min_position_size_usd()
            {
                self.size_delta_usd = self.position.size_in_usd().clone();
            }
        }
        Ok(())
    }

    fn check_close(&mut self) -> crate::Result<()> {
        if self.size_delta_usd == *self.position.size_in_usd()
            && !self.withdrawable_collateral_amount.is_zero()
        {
            // Help ensure that the order can be executed.
            self.withdrawable_collateral_amount = Zero::zero();
        }
        Ok(())
    }

    fn check_liquiation(&self) -> crate::Result<()> {
        if self.params.is_liquidation_order {
            let Some(_reason) = self
                .position
                .check_liquidatable(&self.params.prices, true)?
            else {
                return Err(crate::Error::NotLiquidatable);
            };
            Ok(())
        } else {
            Ok(())
        }
    }

    fn is_partial_close(&self) -> bool {
        self.size_delta_usd < *self.position.size_in_usd()
    }

    fn is_fully_close(&self) -> bool {
        self.size_delta_usd == *self.position.size_in_usd()
    }

    fn collateral_token_price(&self) -> &Price<P::Num> {
        self.position.collateral_price(self.params.prices())
    }

    #[allow(clippy::type_complexity)]
    fn process_collateral(&mut self) -> crate::Result<ProcessCollateralResult<P::Num>> {
        use num_traits::Signed;

        let is_insolvent_close_allowed =
            self.params.is_insolvent_close_allowed && self.is_fully_close();

        let (price_impact_value, price_impact_diff, execution_price) =
            self.get_execution_params()?;

        // Calculate position pnl usd.
        let (base_pnl_usd, uncapped_base_pnl_usd, size_delta_in_tokens) = self
            .position
            .pnl_value(&self.params.prices, &self.size_delta_usd)?;

        let is_output_token_long = self.position.is_collateral_token_long();
        let is_pnl_token_long = self.position.is_long();
        let are_pnl_and_collateral_tokens_the_same =
            self.position.are_pnl_and_collateral_tokens_the_same();

        let mut fees = self.position.position_fees(
            self.params
                .prices
                .collateral_token_price(is_output_token_long),
            &self.size_delta_usd,
            price_impact_value.is_positive(),
        )?;

        let remaining_collateral_amount = self.position.collateral_amount().clone();
        let processor = CollateralProcessor::new(
            self.position.market_mut(),
            is_output_token_long,
            is_pnl_token_long,
            are_pnl_and_collateral_tokens_the_same,
            &self.params.prices,
            remaining_collateral_amount,
            is_insolvent_close_allowed,
        );
        let mut report = processor.process(|mut ctx| {
            ctx.add_pnl_if_positive(&base_pnl_usd)?
                .add_price_impact_if_positive(&price_impact_value)?
                .pay_for_funding_fees(fees.funding_fees())?
                .pay_for_pnl_if_negative(&base_pnl_usd)?
                .pay_for_fees_excluding_funding(&mut fees)?
                .pay_for_price_impact_if_negative(&price_impact_value)?
                .pay_for_price_impact_diff(&price_impact_diff)?;
            Ok(())
        })?;

        // Handle initial collateral delta amount with price impact diff.
        // FIXME: Comment on the reason.
        if !self.withdrawable_collateral_amount.is_zero() && !price_impact_diff.is_zero() {
            // The prices should have been validated to be non-zero.
            debug_assert!(!self.collateral_token_price().has_zero());
            let diff_amount =
                price_impact_diff.clone() / self.collateral_token_price().pick_price(false).clone();
            if self.withdrawable_collateral_amount > diff_amount {
                self.withdrawable_collateral_amount = self
                    .withdrawable_collateral_amount
                    .checked_sub(&diff_amount)
                    .ok_or(crate::Error::Computation(
                        "calculating new withdrawable amount",
                    ))?;
            } else {
                self.withdrawable_collateral_amount = P::Num::zero();
            }
        }

        // Cap the withdrawal amount to the remaining collateral amount.
        if self.withdrawable_collateral_amount > report.remaining_collateral_amount {
            self.withdrawable_collateral_amount = report.remaining_collateral_amount.clone();
        }

        if !self.withdrawable_collateral_amount.is_zero() {
            report.remaining_collateral_amount = report
                .remaining_collateral_amount
                .checked_sub(&self.withdrawable_collateral_amount)
                .expect("must be success");
            report.output_amount = report
                .output_amount
                .checked_add(&self.withdrawable_collateral_amount)
                .ok_or(crate::Error::Computation(
                    "overflow adding withdrawable amount",
                ))?;
        }

        Ok(ProcessCollateralResult {
            price_impact_value,
            price_impact_diff,
            execution_price,
            size_delta_in_tokens,
            is_output_token_long,
            is_secondary_output_token_long: is_pnl_token_long,
            collateral: report,
            fees,
            pnl: Pnl::new(base_pnl_usd, uncapped_base_pnl_usd),
        })
    }

    fn get_execution_params(&self) -> crate::Result<(P::Signed, P::Num, P::Num)> {
        let index_token_price = &self.params.prices.index_token_price;
        let size_delta_usd = &self.size_delta_usd;

        if size_delta_usd.is_zero() {
            return Ok((
                Zero::zero(),
                Zero::zero(),
                index_token_price
                    .pick_price(!self.position.is_long())
                    .clone(),
            ));
        }

        let (price_impact_value, price_impact_diff_usd) =
            self.position.capped_position_price_impact(
                index_token_price,
                &self.size_delta_usd.to_opposite_signed()?,
            )?;

        let execution_price = utils::get_execution_price_for_decrease(
            index_token_price,
            self.position.size_in_usd(),
            self.position.size_in_tokens(),
            size_delta_usd,
            &price_impact_value,
            self.params.acceptable_price.as_ref(),
            self.position.is_long(),
        )?;

        Ok((price_impact_value, price_impact_diff_usd, execution_price))
    }

    /// Swap the secondary output tokens to output tokens if needed.
    fn swap_output_tokens_if_needed(
        market: &mut P::Market,
        report: &mut DecreasePositionReport<P::Num>,
        prices: &Prices<P::Num>,
        are_pnl_and_collateral_tokens_the_same: bool,
    ) -> crate::Result<()> {
        let is_secondary_output_token_long = report.is_secondary_output_token_long();
        let (output_amount, secondary_output_amount) = report.output_amounts_mut();
        if !secondary_output_amount.is_zero() {
            if are_pnl_and_collateral_tokens_the_same {
                *output_amount = output_amount
                    .checked_add(secondary_output_amount)
                    .ok_or(crate::Error::Computation("merging output tokens"))?;
                *secondary_output_amount = Zero::zero();
            } else {
                let swap_report = market
                    .swap(
                        is_secondary_output_token_long,
                        secondary_output_amount.clone(),
                        prices.clone(),
                    )?
                    .execute()?;
                *output_amount = output_amount
                    .checked_add(swap_report.token_out_amount())
                    .ok_or(crate::Error::Computation("adding swapped output tokens"))?;
                *secondary_output_amount = Zero::zero();
                report.set_swap_output_tokens_report(swap_report);
            }
        }
        Ok(())
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
                80_000_000_000,
                None,
            )?
            .execute()?;
        println!("{report:#?}");
        println!("{position:#?}");

        let report = position
            .ops(&mut market)
            .decrease(
                Prices::new_for_test(125, 125, 1),
                40_000_000_000,
                None,
                100_000_000,
                false,
                false,
            )?
            .execute()?;
        println!("{report:#?}");
        println!("{position:#?}");
        println!("{market:#?}");

        let report = position
            .ops(&mut market)
            .decrease(
                Prices::new_for_test(118, 118, 1),
                40_000_000_000,
                None,
                0,
                false,
                false,
            )?
            .execute()?;
        println!("{report:#?}");
        println!("{position:#?}");
        println!("{market:#?}");
        Ok(())
    }
}
