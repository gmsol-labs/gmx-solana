use num_traits::{CheckedAdd, Zero};
use typed_builder::TypedBuilder;

use crate::{fixed::FixedPointOps, num::Unsigned, price::Price, utils};

/// Fee Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct FeeParams<T> {
    positive_impact_fee_factor: T,
    negative_impact_fee_factor: T,
    fee_receiver_factor: T,
    #[builder(default = None, setter(strip_option))]
    discount_factor: Option<T>,
}

impl<T> FeeParams<T> {
    /// Set discount factor.
    pub fn with_discount_factor(self, factor: T) -> Self {
        Self {
            discount_factor: Some(factor),
            ..self
        }
    }

    #[inline]
    fn factor(&self, is_positive_impact: bool) -> &T {
        if is_positive_impact {
            &self.positive_impact_fee_factor
        } else {
            &self.negative_impact_fee_factor
        }
    }

    fn discount_factor(&self) -> T
    where
        T: Zero + Clone,
    {
        self.discount_factor
            .as_ref()
            .cloned()
            .unwrap_or(Zero::zero())
    }

    /// Get basic fee.
    #[inline]
    pub fn fee<const DECIMALS: u8>(&self, is_positive_impact: bool, amount: &T) -> Option<T>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let factor = self.factor(is_positive_impact);
        let fee = utils::apply_factor(amount, factor)?;
        let discount = utils::apply_factor(&fee, &self.discount_factor())?;
        fee.checked_sub(&discount)
    }

    /// Get receiver fee.
    #[inline]
    pub fn receiver_fee<const DECIMALS: u8>(&self, fee_amount: &T) -> Option<T>
    where
        T: FixedPointOps<DECIMALS>,
    {
        utils::apply_factor(fee_amount, &self.fee_receiver_factor)
    }

    /// Apply fees to `amount`.
    /// - `DECIMALS` is the decimals of the parameters.
    ///
    /// Returns `None` if the computation fails, otherwise `amount` after fees and the fees are returned.
    pub fn apply_fees<const DECIMALS: u8>(
        &self,
        is_positive_impact: bool,
        amount: &T,
    ) -> Option<(T, Fees<T>)>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let fee_amount = self.fee(is_positive_impact, amount)?;
        let fee_receiver_amount = self.receiver_fee(&fee_amount)?;
        let fees = Fees {
            fee_amount_for_pool: fee_amount.checked_sub(&fee_receiver_amount)?,
            fee_receiver_amount,
        };
        Some((amount.checked_sub(&fee_amount)?, fees))
    }

    /// Get order fees.
    fn order_fees<const DECIMALS: u8>(
        &self,
        collateral_token_price: &Price<T>,
        size_delta_usd: &T,
        is_positive_impact: bool,
    ) -> crate::Result<OrderFees<T>>
    where
        T: FixedPointOps<DECIMALS>,
    {
        if collateral_token_price.has_zero() {
            return Err(crate::Error::InvalidPrices);
        }

        let fee_amount = self
            .fee(is_positive_impact, size_delta_usd)
            .ok_or(crate::Error::Computation("calculating order fee usd"))?
            / collateral_token_price.pick_price(false).clone();

        let receiver_fee_amount = self
            .receiver_fee(&fee_amount)
            .ok_or(crate::Error::Computation("calculating order receiver fee"))?;
        Ok(OrderFees {
            base: Fees::new(
                fee_amount
                    .checked_sub(&receiver_fee_amount)
                    .ok_or(crate::Error::Computation("calculating order fee for pool"))?,
                receiver_fee_amount,
            ),
        })
    }

    /// Get base position fees.
    pub fn base_position_fees<const DECIMALS: u8>(
        &self,
        collateral_token_price: &Price<T>,
        size_delta_usd: &T,
        is_positive_impact: bool,
    ) -> crate::Result<PositionFees<T>>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let OrderFees { base } =
            self.order_fees(collateral_token_price, size_delta_usd, is_positive_impact)?;
        Ok(PositionFees {
            base,
            borrowing: Default::default(),
            funding: Default::default(),
        })
    }
}

/// Borrowing Fee Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct BorrowingFeeParams<T> {
    receiver_factor: T,
    exponent_for_long: T,
    exponent_for_short: T,
    factor_for_long: T,
    factor_for_short: T,
    #[builder(default = true)]
    skip_borrowing_fee_for_smaller_side: bool,
}

impl<T> BorrowingFeeParams<T> {
    /// Get borrowing exponent factor.
    pub fn exponent(&self, is_long: bool) -> &T {
        if is_long {
            &self.exponent_for_long
        } else {
            &self.exponent_for_short
        }
    }

    /// Get borrowing factor.
    pub fn factor(&self, is_long: bool) -> &T {
        if is_long {
            &self.factor_for_long
        } else {
            &self.factor_for_short
        }
    }

    /// Get whether to skip borrowing fee for smaller side.
    pub fn skip_borrowing_fee_for_smaller_side(&self) -> bool {
        self.skip_borrowing_fee_for_smaller_side
    }

    /// Get borrowing fee receiver factor.
    pub fn receiver_factor(&self) -> &T {
        &self.receiver_factor
    }
}

/// Funding Fee Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct FundingFeeParams<T> {
    exponent: T,
    funding_factor: T,
    increase_factor_per_second: T,
    decrease_factor_per_second: T,
    max_factor_per_second: T,
    min_factor_per_second: T,
    threshold_for_stable_funding: T,
    threshold_for_decrease_funding: T,
}

impl<T> FundingFeeParams<T> {
    /// Get funding exponent factor.
    pub fn exponent(&self) -> &T {
        &self.exponent
    }

    /// Get funding increase factor per second.
    pub fn increase_factor_per_second(&self) -> &T {
        &self.increase_factor_per_second
    }

    /// Get funding decrease factor per second.
    pub fn decrease_factor_per_second(&self) -> &T {
        &self.decrease_factor_per_second
    }

    /// Get max funding factor per second.
    pub fn max_factor_per_second(&self) -> &T {
        &self.max_factor_per_second
    }

    /// Get min funding factor per second.
    pub fn min_factor_per_second(&self) -> &T {
        &self.min_factor_per_second
    }

    /// Fallback funding factor.
    pub fn factor(&self) -> &T {
        &self.funding_factor
    }

    /// Threshold for stable funding.
    pub fn threshold_for_stable_funding(&self) -> &T {
        &self.threshold_for_stable_funding
    }

    /// Threshold for decrease funding.
    pub fn threshold_for_decrease_funding(&self) -> &T {
        &self.threshold_for_decrease_funding
    }

    /// Get change type for next funding rate.
    pub fn change(
        &self,
        funding_factor_per_second: &T::Signed,
        long_open_interest: &T,
        short_open_interest: &T,
        diff_factor: &T,
    ) -> FundingRateChangeType
    where
        T: Ord + Unsigned,
    {
        use num_traits::Signed;

        let is_skew_the_same_direction_as_funding = (funding_factor_per_second.is_positive()
            && *long_open_interest > *short_open_interest)
            || (funding_factor_per_second.is_negative()
                && *long_open_interest < *short_open_interest);

        if is_skew_the_same_direction_as_funding {
            if *diff_factor > self.threshold_for_stable_funding {
                FundingRateChangeType::Increase
            } else if *diff_factor < self.threshold_for_decrease_funding {
                FundingRateChangeType::Decrease
            } else {
                FundingRateChangeType::NoChange
            }
        } else {
            FundingRateChangeType::Increase
        }
    }
}

/// Funding Rate Change Type.
#[derive(Default, Debug)]
pub enum FundingRateChangeType {
    /// No Change.
    #[default]
    NoChange,
    /// Increase.
    Increase,
    /// Decrease.
    Decrease,
}

/// Fees.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
pub struct Fees<T> {
    fee_receiver_amount: T,
    fee_amount_for_pool: T,
}

impl<T: Zero> Default for Fees<T> {
    fn default() -> Self {
        Self {
            fee_receiver_amount: Zero::zero(),
            fee_amount_for_pool: Zero::zero(),
        }
    }
}

impl<T> Fees<T> {
    /// Create a new [`Fees`].
    pub fn new(pool: T, receiver: T) -> Self {
        Self {
            fee_amount_for_pool: pool,
            fee_receiver_amount: receiver,
        }
    }

    /// Get fee receiver amount.
    pub fn fee_receiver_amount(&self) -> &T {
        &self.fee_receiver_amount
    }

    /// Get fee amount for pool.
    pub fn fee_amount_for_pool(&self) -> &T {
        &self.fee_amount_for_pool
    }
}

/// Order Fees.
pub struct OrderFees<T> {
    base: Fees<T>,
}

/// Borrowing Fee.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy)]
pub struct BorrowingFee<T> {
    amount: T,
    amount_for_receiver: T,
}

impl<T> BorrowingFee<T> {
    /// Get total borrowing fee amount.
    pub fn amount(&self) -> &T {
        &self.amount
    }

    /// Get borrowing fee amount for receiver.
    pub fn amount_for_receiver(&self) -> &T {
        &self.amount_for_receiver
    }
}

impl<T: Zero> Default for BorrowingFee<T> {
    fn default() -> Self {
        Self {
            amount: Zero::zero(),
            amount_for_receiver: Zero::zero(),
        }
    }
}

/// Funding Fees.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct FundingFees<T> {
    amount: T,
    claimable_long_token_amount: T,
    claimable_short_token_amount: T,
}

impl<T> FundingFees<T> {
    /// Get funding fee amount.
    pub fn amount(&self) -> &T {
        &self.amount
    }

    /// Get claimable long token funding fee amount.
    pub fn claimable_long_token_amount(&self) -> &T {
        &self.claimable_long_token_amount
    }

    /// Get claimble short token funding fee amount.
    pub fn claimable_short_token_amount(&self) -> &T {
        &self.claimable_short_token_amount
    }
}

impl<T: Zero> Default for FundingFees<T> {
    fn default() -> Self {
        Self {
            amount: Zero::zero(),
            claimable_long_token_amount: Zero::zero(),
            claimable_short_token_amount: Zero::zero(),
        }
    }
}

/// Position Fees.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy)]
pub struct PositionFees<T> {
    base: Fees<T>,
    borrowing: BorrowingFee<T>,
    funding: FundingFees<T>,
}

impl<T> PositionFees<T> {
    /// Get fee for receiver.
    pub fn for_receiver(&self) -> crate::Result<T>
    where
        T: CheckedAdd,
    {
        self.base
            .fee_receiver_amount
            .checked_add(self.borrowing.amount_for_receiver())
            .ok_or(crate::Error::Computation("calculating fee for receiver"))
    }

    /// Get fee for pool.
    pub fn for_pool<const DECIMALS: u8>(&self) -> crate::Result<T>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let amount = self
            .base
            .fee_amount_for_pool
            .checked_add(&self.borrowing.amount)
            .and_then(|total| total.checked_sub(self.borrowing.amount_for_receiver()))
            .ok_or(crate::Error::Computation("calculating fee for pool"))?;
        Ok(amount)
    }

    /// Get order fee.
    pub fn order_fee(&self) -> &Fees<T> {
        &self.base
    }

    /// Get borrowing fee.
    pub fn borrowing(&self) -> &BorrowingFee<T> {
        &self.borrowing
    }

    /// Get funding fees.
    pub fn funding_fees(&self) -> &FundingFees<T> {
        &self.funding
    }

    /// Get total cost amount in collateral tokens.
    pub fn total_cost_amount(&self) -> crate::Result<T>
    where
        T: CheckedAdd,
    {
        self.total_cost_excluding_funding()?
            .checked_add(&self.funding.amount)
            .ok_or(crate::Error::Overflow)
    }

    /// Get total cost excluding funding fee.
    pub fn total_cost_excluding_funding(&self) -> crate::Result<T>
    where
        T: CheckedAdd,
    {
        self.base
            .fee_amount_for_pool
            .checked_add(&self.base.fee_receiver_amount)
            .and_then(|acc| acc.checked_add(&self.borrowing.amount))
            .ok_or(crate::Error::Overflow)
    }

    /// Clear fees excluding funding fee.
    pub fn clear_fees_excluding_funding(&mut self)
    where
        T: Zero,
    {
        self.base = Fees::default();
        self.borrowing = BorrowingFee::default();
    }

    /// Apply borrowing fee.
    pub fn apply_borrowing_fee<const DECIMALS: u8>(
        mut self,
        receiver_factor: &T,
        price: &Price<T>,
        value: T,
    ) -> crate::Result<Self>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let price = price.pick_price(false);
        debug_assert!(!price.is_zero(), "must be non-zero");
        let amount = value / price.clone();
        self.borrowing.amount_for_receiver = crate::utils::apply_factor(&amount, receiver_factor)
            .ok_or(crate::Error::Computation(
            "calculating borrowing fee amount for receiver",
        ))?;
        self.borrowing.amount = amount;
        Ok(self)
    }

    /// Apply funding fees.
    pub fn apply_funding_fees(mut self, fees: FundingFees<T>) -> Self {
        self.funding = fees;
        self
    }
}

impl<T: Zero> Default for PositionFees<T> {
    fn default() -> Self {
        Self {
            base: Default::default(),
            borrowing: Default::default(),
            funding: Default::default(),
        }
    }
}
