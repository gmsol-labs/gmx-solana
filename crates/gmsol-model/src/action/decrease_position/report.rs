use std::fmt;

use crate::{
    action::{
        swap::SwapReport, update_borrowing_state::UpdateBorrowingReport,
        update_funding_state::UpdateFundingReport,
    },
    num::Unsigned,
    params::fee::PositionFees,
    position::InsolventCloseStep,
};

use super::{ClaimableCollateral, DecreasePositionParams, ProcessCollateralResult};

/// Report of the execution of position decreasing.
#[must_use]
pub struct DecreasePositionReport<T: Unsigned> {
    price_impact_value: T::Signed,
    price_impact_diff: T,
    execution_price: T,
    size_delta_in_tokens: T,
    withdrawable_collateral_amount: T,
    initial_size_delta_usd: T,
    size_delta_usd: T,
    fees: PositionFees<T>,
    borrowing: UpdateBorrowingReport<T>,
    funding: UpdateFundingReport<T>,
    swap_output_tokens: Option<SwapReport<T>>,
    pnl: Pnl<T::Signed>,
    insolvent_close_step: Option<InsolventCloseStep>,
    // Output
    should_remove: bool,
    is_output_token_long: bool,
    is_secondary_output_token_long: bool,
    output_amounts: OutputAmounts<T>,
    claimable_funding_long_token_amount: T,
    claimable_funding_short_token_amount: T,
    for_holding: ClaimableCollateral<T>,
    for_user: ClaimableCollateral<T>,
}

impl<T: Unsigned + fmt::Debug> fmt::Debug for DecreasePositionReport<T>
where
    T::Signed: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecreasePositionReport")
            .field("price_impact_value", &self.price_impact_value)
            .field("price_impact_diff", &self.price_impact_diff)
            .field("execution_price", &self.execution_price)
            .field("size_delta_in_tokens", &self.size_delta_in_tokens)
            .field(
                "withdrawable_collateral_amount",
                &self.withdrawable_collateral_amount,
            )
            .field("initial_size_delta_usd", &self.initial_size_delta_usd)
            .field("size_delta_usd", &self.size_delta_usd)
            .field("fees", &self.fees)
            .field("borrowing", &self.borrowing)
            .field("funding", &self.funding)
            .field("swap_output_tokens", &self.swap_output_tokens)
            .field("pnl", &self.pnl)
            .field("insolvent_close_step", &self.insolvent_close_step)
            .field("should_remove", &self.should_remove)
            .field("is_output_token_long", &self.is_output_token_long)
            .field(
                "is_secondary_output_token_long",
                &self.is_secondary_output_token_long,
            )
            .field("output_amounts", &self.output_amounts)
            .field(
                "claimable_funding_long_token_amount",
                &self.claimable_funding_long_token_amount,
            )
            .field(
                "claimable_funding_short_token_amount",
                &self.claimable_funding_short_token_amount,
            )
            .field("for_holding", &self.for_holding)
            .field("for_user", &self.for_user)
            .finish()
    }
}

impl<T: Unsigned + Clone> DecreasePositionReport<T> {
    pub(super) fn new(
        params: &DecreasePositionParams<T>,
        execution: ProcessCollateralResult<T>,
        withdrawable_collateral_amount: T,
        size_delta_usd: T,
        borrowing: UpdateBorrowingReport<T>,
        funding: UpdateFundingReport<T>,
        should_remove: bool,
    ) -> Self {
        let claimable_funding_long_token_amount = execution
            .fees
            .funding_fees()
            .claimable_long_token_amount()
            .clone();
        let claimable_funding_short_token_amount = execution
            .fees
            .funding_fees()
            .claimable_short_token_amount()
            .clone();
        Self {
            price_impact_value: execution.price_impact_value,
            price_impact_diff: execution.price_impact_diff,
            execution_price: execution.execution_price,
            size_delta_in_tokens: execution.size_delta_in_tokens,
            withdrawable_collateral_amount,
            initial_size_delta_usd: params.initial_size_delta_usd.clone(),
            size_delta_usd,
            fees: execution.fees,
            borrowing,
            funding,
            swap_output_tokens: None,
            pnl: execution.pnl,
            insolvent_close_step: execution.collateral.insolvent_close_step,
            // Output
            should_remove,
            is_output_token_long: execution.is_output_token_long,
            is_secondary_output_token_long: execution.is_secondary_output_token_long,
            output_amounts: OutputAmounts {
                output_amount: execution.collateral.output_amount,
                secondary_output_amount: execution.collateral.secondary_output_amount,
            },
            claimable_funding_long_token_amount,
            claimable_funding_short_token_amount,
            for_holding: execution.collateral.for_holding,
            for_user: execution.collateral.for_user,
        }
    }

    pub(super) fn set_swap_output_tokens_report(&mut self, report: SwapReport<T>) {
        self.swap_output_tokens = Some(report);
    }

    /// Get size delta in tokens.
    pub fn size_delta_in_tokens(&self) -> &T {
        &self.size_delta_in_tokens
    }

    /// Get initial size delta in usd.
    pub fn initial_size_delta_usd(&self) -> &T {
        &self.initial_size_delta_usd
    }

    /// Get size delta in usd.
    pub fn size_delta_usd(&self) -> &T {
        &self.size_delta_usd
    }

    /// Get execution price.
    pub fn execution_price(&self) -> &T {
        &self.execution_price
    }

    /// Get price impact value.
    pub fn price_impact_value(&self) -> &T::Signed {
        &self.price_impact_value
    }

    /// Get price impact diff.
    pub fn price_impact_diff(&self) -> &T {
        &self.price_impact_diff
    }

    /// Get execution fees.
    pub fn fees(&self) -> &PositionFees<T> {
        &self.fees
    }

    /// Returns whether the output token (collateral token) is long token.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn is_output_token_long(&self) -> bool {
        self.is_output_token_long
    }

    /// Returns whether the secondary output token (pnl token) is long token.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn is_secondary_output_token_long(&self) -> bool {
        self.is_secondary_output_token_long
    }

    /// Get output amount.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn output_amount(&self) -> &T {
        &self.output_amounts.output_amount
    }

    /// Get secondary output amount.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn secondary_output_amount(&self) -> &T {
        &self.output_amounts.secondary_output_amount
    }

    /// Get output amounts.
    pub fn output_amounts(&self) -> &OutputAmounts<T> {
        &self.output_amounts
    }

    pub(super) fn output_amounts_mut(&mut self) -> (&mut T, &mut T) {
        (
            &mut self.output_amounts.output_amount,
            &mut self.output_amounts.secondary_output_amount,
        )
    }

    /// Get should remove.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn should_remove(&self) -> bool {
        self.should_remove
    }

    /// Get withdrawable collateral amount.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn withdrawable_collateral_amount(&self) -> &T {
        &self.withdrawable_collateral_amount
    }

    /// Get claimable funding amounts.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn claimable_funding_amounts(&self) -> (&T, &T) {
        (
            &self.claimable_funding_long_token_amount,
            &self.claimable_funding_short_token_amount,
        )
    }

    /// Get claimable collateral for holding.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn claimable_collateral_for_holding(&self) -> &ClaimableCollateral<T> {
        &self.for_holding
    }

    /// Get Get claimable collateral for user.
    ///
    /// ## Must Use
    /// Must be used by the caller.
    #[must_use]
    pub fn claimable_collateral_for_user(&self) -> &ClaimableCollateral<T> {
        &self.for_user
    }

    /// Get borrowing report.
    pub fn borrowing(&self) -> &UpdateBorrowingReport<T> {
        &self.borrowing
    }

    /// Get funding report.
    pub fn funding(&self) -> &UpdateFundingReport<T> {
        &self.funding
    }

    /// Get the report of output tokens swap.
    pub fn swap_output_tokens(&self) -> Option<&SwapReport<T>> {
        self.swap_output_tokens.as_ref()
    }

    /// Get processed pnl.
    pub fn pnl(&self) -> &Pnl<T::Signed> {
        &self.pnl
    }

    /// Get insolvent close step.
    pub fn insolvent_close_step(&self) -> Option<InsolventCloseStep> {
        self.insolvent_close_step
    }
}

/// Processed PnL.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Pnl<T> {
    /// Final PnL value.
    pnl: T,
    /// Uncapped PnL value.
    uncapped_pnl: T,
}

impl<T> Pnl<T> {
    /// Create a new [`Pnl`].
    pub fn new(pnl: T, uncapped_pnl: T) -> Self {
        Self { pnl, uncapped_pnl }
    }

    /// Get final pnl value.
    pub fn pnl(&self) -> &T {
        &self.pnl
    }

    /// Get uncapped pnl value.
    pub fn uncapped_pnl(&self) -> &T {
        &self.uncapped_pnl
    }
}

/// Output amounts.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputAmounts<T> {
    pub(super) output_amount: T,
    pub(super) secondary_output_amount: T,
}

impl<T> OutputAmounts<T> {
    /// Get output amount.
    pub fn output_amount(&self) -> &T {
        &self.output_amount
    }

    /// Get secondary output amount.
    pub fn secondary_output_amount(&self) -> &T {
        &self.secondary_output_amount
    }
}
