use num_traits::{CheckedAdd, CheckedSub, Zero};
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
            fee_amount_for_receiver: fee_receiver_amount,
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

        let fee_value = self
            .fee(is_positive_impact, size_delta_usd)
            .ok_or(crate::Error::Computation("calculating order fee value"))?;
        let fee_amount = fee_value
            .checked_div(collateral_token_price.pick_price(false))
            .ok_or(crate::Error::Computation("calculating order fee amount"))?;

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
            fee_value,
        })
    }

    /// Get position fees with only order fees considered.
    pub fn base_position_fees<const DECIMALS: u8>(
        &self,
        collateral_token_price: &Price<T>,
        size_delta_usd: &T,
        is_positive_impact: bool,
    ) -> crate::Result<PositionFees<T>>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let order_fees =
            self.order_fees(collateral_token_price, size_delta_usd, is_positive_impact)?;
        Ok(PositionFees {
            paid_order_fee_value: order_fees.fee_value.clone(),
            order: order_fees,
            borrowing: Default::default(),
            funding: Default::default(),
            liquidation: Default::default(),
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

/// Borrowing Fee Kink Model Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct BorrowingFeeKinkModelParams<T> {
    long: BorrowingFeeKinkModelParamsForOneSide<T>,
    short: BorrowingFeeKinkModelParamsForOneSide<T>,
}

impl<T> BorrowingFeeKinkModelParams<T> {
    fn params_for_one_side(&self, is_long: bool) -> &BorrowingFeeKinkModelParamsForOneSide<T> {
        if is_long {
            &self.long
        } else {
            &self.short
        }
    }

    /// Get optimal usage factor.
    pub fn optimal_usage_factor(&self, is_long: bool) -> &T {
        &self.params_for_one_side(is_long).optimal_usage_factor
    }

    /// Get base borrowing factor.
    pub fn base_borrowing_factor(&self, is_long: bool) -> &T {
        &self.params_for_one_side(is_long).base_borrowing_factor
    }

    /// Get above optimal usage borrowing factor.
    pub fn above_optimal_usage_borrowing_factor(&self, is_long: bool) -> &T {
        &self
            .params_for_one_side(is_long)
            .above_optimal_usage_borrowing_factor
    }

    /// Calculate borrowing factor per second.
    pub fn borrowing_factor_per_second<const DECIMALS: u8, M>(
        &self,
        market: &M,
        is_long: bool,
        reserved_value: &T,
        pool_value: &T,
    ) -> crate::Result<Option<T>>
    where
        M: crate::BaseMarket<DECIMALS, Num = T> + ?Sized,
        T: FixedPointOps<DECIMALS>,
    {
        use crate::market::utils::MarketUtils;

        let optimal_usage_factor = self.optimal_usage_factor(is_long);

        if optimal_usage_factor.is_zero() {
            return Ok(None);
        }

        let usage_factor = market.usage_factor(is_long, reserved_value, pool_value)?;

        let base_borrowing_factor = self.base_borrowing_factor(is_long);

        let borrowing_factor_per_second = utils::apply_factor(&usage_factor, base_borrowing_factor)
            .ok_or(crate::Error::Computation(
                "borrowing fee kink model: calculating borrowing factor per second",
            ))?;

        if usage_factor > *optimal_usage_factor && T::UNIT > *optimal_usage_factor {
            let diff =
                usage_factor
                    .checked_sub(optimal_usage_factor)
                    .ok_or(crate::Error::Computation(
                        "borrowing fee kink model: calculating diff",
                    ))?;

            let above_optimal_usage_borrowing_factor =
                self.above_optimal_usage_borrowing_factor(is_long);

            let additional_borrowing_factor_per_second =
                if above_optimal_usage_borrowing_factor > base_borrowing_factor {
                    above_optimal_usage_borrowing_factor
                        .checked_sub(base_borrowing_factor)
                        .ok_or(crate::Error::Computation(
                            "borrowing fee kink model: calculating addtional factor",
                        ))?
                } else {
                    T::zero()
                };

            let divisor =
                T::UNIT
                    .checked_sub(optimal_usage_factor)
                    .ok_or(crate::Error::Computation(
                        "borrowing fee kink model: calculating divisor",
                    ))?;

            let borrowing_factor_per_second = additional_borrowing_factor_per_second
                .checked_mul_div(&diff, &divisor)
                .and_then(|a| borrowing_factor_per_second.checked_add(&a))
                .ok_or(crate::Error::Computation(
                    "borrowing fee kink model: increasing borrowing factor per second",
                ))?;

            Ok(Some(borrowing_factor_per_second))
        } else {
            Ok(Some(borrowing_factor_per_second))
        }
    }
}

/// Borrowing Fee Kink Model Parameters for one side.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct BorrowingFeeKinkModelParamsForOneSide<T> {
    optimal_usage_factor: T,
    base_borrowing_factor: T,
    above_optimal_usage_borrowing_factor: T,
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

/// Liquidation Fee Parameters.
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub struct LiquidationFeeParams<T> {
    factor: T,
    receiver_factor: T,
}

impl<T> LiquidationFeeParams<T> {
    pub(crate) fn fee<const DECIMALS: u8>(
        &self,
        size_delta_usd: &T,
        collateral_token_price: &Price<T>,
    ) -> crate::Result<LiquidationFees<T>>
    where
        T: FixedPointOps<DECIMALS>,
    {
        if self.factor.is_zero() {
            return Ok(Default::default());
        }

        let fee_value = utils::apply_factor(size_delta_usd, &self.factor).ok_or(
            crate::Error::Computation("liquidation fee: calculating fee value"),
        )?;
        let fee_amount = fee_value
            .checked_round_up_div(collateral_token_price.pick_price(false))
            .ok_or(crate::Error::Computation(
                "liquidation fee: calculating fee amount",
            ))?;
        let fee_amount_for_receiver = utils::apply_factor(&fee_amount, &self.receiver_factor)
            .ok_or(crate::Error::Computation(
                "liquidation fee: calculating fee amount for receiver",
            ))?;

        Ok(LiquidationFees {
            fee_value,
            fee_amount,
            fee_amount_for_receiver,
        })
    }
}

/// Fees.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
pub struct Fees<T> {
    fee_amount_for_receiver: T,
    fee_amount_for_pool: T,
}

impl<T: Zero> Default for Fees<T> {
    fn default() -> Self {
        Self {
            fee_amount_for_receiver: Zero::zero(),
            fee_amount_for_pool: Zero::zero(),
        }
    }
}

impl<T> Fees<T> {
    /// Create a new [`Fees`].
    pub fn new(pool: T, receiver: T) -> Self {
        Self {
            fee_amount_for_pool: pool,
            fee_amount_for_receiver: receiver,
        }
    }

    /// Get fee amount for receiver
    pub fn fee_amount_for_receiver(&self) -> &T {
        &self.fee_amount_for_receiver
    }

    /// Get fee amount for pool.
    pub fn fee_amount_for_pool(&self) -> &T {
        &self.fee_amount_for_pool
    }
}

/// Order Fees.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy)]
pub struct OrderFees<T> {
    base: Fees<T>,
    fee_value: T,
}

impl<T> OrderFees<T> {
    /// Get fee amounts.
    pub fn fee_amounts(&self) -> &Fees<T> {
        &self.base
    }

    /// Get order fee value.
    pub fn fee_value(&self) -> &T {
        &self.fee_value
    }
}

impl<T: Zero> Default for OrderFees<T> {
    fn default() -> Self {
        Self {
            base: Default::default(),
            fee_value: Zero::zero(),
        }
    }
}

/// Borrowing Fee.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy)]
pub struct BorrowingFees<T> {
    fee_amount: T,
    fee_amount_for_receiver: T,
}

impl<T> BorrowingFees<T> {
    /// Get total borrowing fee amount.
    pub fn fee_amount(&self) -> &T {
        &self.fee_amount
    }

    /// Get borrowing fee amount for receiver.
    pub fn fee_amount_for_receiver(&self) -> &T {
        &self.fee_amount_for_receiver
    }

    /// Get borrowing fee amount for pool.
    pub fn fee_amount_for_pool(&self) -> crate::Result<T>
    where
        T: CheckedSub,
    {
        self.fee_amount
            .checked_sub(&self.fee_amount_for_receiver)
            .ok_or(crate::Error::Computation(
                "borrowing fee: calculating fee for pool",
            ))
    }
}

impl<T: Zero> Default for BorrowingFees<T> {
    fn default() -> Self {
        Self {
            fee_amount: Zero::zero(),
            fee_amount_for_receiver: Zero::zero(),
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

/// Liquidation Fees.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
#[derive(Debug, Clone, Copy)]
pub struct LiquidationFees<T> {
    fee_value: T,
    fee_amount: T,
    fee_amount_for_receiver: T,
}

impl<T: Zero> Default for LiquidationFees<T> {
    fn default() -> Self {
        Self {
            fee_value: Zero::zero(),
            fee_amount: Zero::zero(),
            fee_amount_for_receiver: Zero::zero(),
        }
    }
}

impl<T> LiquidationFees<T> {
    /// Get total liquidation fee amount.
    pub fn fee_amount(&self) -> &T {
        &self.fee_amount
    }

    /// Get liquidation fee amount for receiver.
    pub fn fee_amount_for_receiver(&self) -> &T {
        &self.fee_amount_for_receiver
    }

    /// Get liquidation fee amount for pool.
    pub fn fee_amount_for_pool(&self) -> crate::Result<T>
    where
        T: CheckedSub,
    {
        self.fee_amount
            .checked_sub(&self.fee_amount_for_receiver)
            .ok_or(crate::Error::Computation(
                "liquidation fee: calculating fee for pool",
            ))
    }

    /// Get total liquidation fee value.
    pub fn fee_value(&self) -> &T {
        &self.fee_value
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
    paid_order_fee_value: T,
    order: OrderFees<T>,
    borrowing: BorrowingFees<T>,
    funding: FundingFees<T>,
    liquidation: Option<LiquidationFees<T>>,
}

impl<T> PositionFees<T> {
    /// Get fee for receiver.
    pub fn for_receiver(&self) -> crate::Result<T>
    where
        T: CheckedAdd,
    {
        self.order
            .fee_amounts()
            .fee_amount_for_receiver()
            .checked_add(self.borrowing.fee_amount_for_receiver())
            .and_then(|total| {
                if let Some(fees) = self.liquidation_fees() {
                    total.checked_add(fees.fee_amount_for_receiver())
                } else {
                    Some(total)
                }
            })
            .ok_or(crate::Error::Computation("calculating fee for receiver"))
    }

    /// Get fee for pool.
    pub fn for_pool<const DECIMALS: u8>(&self) -> crate::Result<T>
    where
        T: FixedPointOps<DECIMALS>,
    {
        let amount = self
            .order
            .fee_amounts()
            .fee_amount_for_pool()
            .checked_add(&self.borrowing.fee_amount_for_pool()?)
            .ok_or(crate::Error::Computation("adding borrowing fee for pool"))
            .and_then(|total| {
                if let Some(fees) = self.liquidation_fees() {
                    total
                        .checked_add(&fees.fee_amount_for_pool()?)
                        .ok_or(crate::Error::Computation("adding liquidation fee for pool"))
                } else {
                    Ok(total)
                }
            })?;
        Ok(amount)
    }

    /// Get paid order fee value.
    pub fn paid_order_fee_value(&self) -> &T {
        &self.paid_order_fee_value
    }

    /// Get order fees.
    pub fn order_fees(&self) -> &OrderFees<T> {
        &self.order
    }

    /// Get borrowing fees.
    pub fn borrowing_fees(&self) -> &BorrowingFees<T> {
        &self.borrowing
    }

    /// Get funding fees.
    pub fn funding_fees(&self) -> &FundingFees<T> {
        &self.funding
    }

    /// Get liquidation fees.
    pub fn liquidation_fees(&self) -> Option<&LiquidationFees<T>> {
        self.liquidation.as_ref()
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
        self.order
            .fee_amounts()
            .fee_amount_for_pool()
            .checked_add(self.order.fee_amounts().fee_amount_for_receiver())
            .and_then(|acc| acc.checked_add(self.borrowing.fee_amount()))
            .and_then(|acc| {
                if let Some(fees) = self.liquidation_fees() {
                    acc.checked_add(fees.fee_amount())
                } else {
                    Some(acc)
                }
            })
            .ok_or(crate::Error::Computation(
                "overflow while calculating total cost excluding funding",
            ))
    }

    /// Set paid order fee value.
    pub(crate) fn set_paid_order_fee_value(&mut self, paid_order_fee_value: T) {
        self.paid_order_fee_value = paid_order_fee_value;
    }

    /// Clear fees excluding funding fee.
    pub fn clear_fees_excluding_funding(&mut self)
    where
        T: Zero,
    {
        self.order = Default::default();
        self.borrowing = BorrowingFees::default();
        self.liquidation = None;
    }

    /// Set borrowing fees.
    pub fn set_borrowing_fees<const DECIMALS: u8>(
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
        let amount = value
            .checked_div(price)
            .ok_or(crate::Error::Computation("calculating borrowing amount"))?;
        self.borrowing.fee_amount_for_receiver =
            crate::utils::apply_factor(&amount, receiver_factor).ok_or(
                crate::Error::Computation("calculating borrowing fee amount for receiver"),
            )?;
        self.borrowing.fee_amount = amount;
        Ok(self)
    }

    /// Set funding fees.
    pub fn set_funding_fees(mut self, fees: FundingFees<T>) -> Self {
        self.funding = fees;
        self
    }

    /// Set liquidation fees.
    pub fn set_liquidation_fees(mut self, fees: Option<LiquidationFees<T>>) -> Self {
        self.liquidation = fees;
        self
    }
}

impl<T: Zero> Default for PositionFees<T> {
    fn default() -> Self {
        Self {
            paid_order_fee_value: Zero::zero(),
            order: Default::default(),
            borrowing: Default::default(),
            funding: Default::default(),
            liquidation: None,
        }
    }
}
