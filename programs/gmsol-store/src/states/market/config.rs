use anchor_lang::prelude::*;
use bitmaps::Bitmap;

use crate::{constants, states::Factor, CoreError};

/// Max number of config flags.
pub const MAX_CONFIG_FLAGS: usize = 128;

/// Market Flag Value.
pub type MarketConfigFlagValue = u128;

/// Market Flag Bitmap.
pub type MarketConfigFlagBitmap = Bitmap<MAX_CONFIG_FLAGS>;

/// Market Config.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MarketConfig {
    /// Flags.
    flag: MarketConfigFlagValue,
    // Swap impact.
    pub(super) swap_impact_exponent: Factor,
    pub(super) swap_impact_positive_factor: Factor,
    pub(super) swap_impact_negative_factor: Factor,
    // Swap fee.
    pub(super) swap_fee_receiver_factor: Factor,
    pub(super) swap_fee_factor_for_positive_impact: Factor,
    pub(super) swap_fee_factor_for_negative_impact: Factor,
    // Position general.
    pub(super) min_position_size_usd: Factor,
    pub(super) min_collateral_value: Factor,
    pub(super) min_collateral_factor: Factor,
    pub(super) min_collateral_factor_for_open_interest_multiplier_for_long: Factor,
    pub(super) min_collateral_factor_for_open_interest_multiplier_for_short: Factor,
    pub(super) max_positive_position_impact_factor: Factor,
    pub(super) max_negative_position_impact_factor: Factor,
    pub(super) max_position_impact_factor_for_liquidations: Factor,
    // Position impact.
    pub(super) position_impact_exponent: Factor,
    pub(super) position_impact_positive_factor: Factor,
    pub(super) position_impact_negative_factor: Factor,
    // Order fee.
    pub(super) order_fee_receiver_factor: Factor,
    pub(super) order_fee_factor_for_positive_impact: Factor,
    pub(super) order_fee_factor_for_negative_impact: Factor,
    // Liquidation fee.
    pub(super) liquidation_fee_receiver_factor: Factor,
    pub(super) liquidation_fee_factor: Factor,
    // Position impact distribution.
    pub(super) position_impact_distribute_factor: Factor,
    pub(super) min_position_impact_pool_amount: Factor,
    // Borrowing fee.
    pub(super) borrowing_fee_receiver_factor: Factor,
    pub(super) borrowing_fee_factor_for_long: Factor,
    pub(super) borrowing_fee_factor_for_short: Factor,
    pub(super) borrowing_fee_exponent_for_long: Factor,
    pub(super) borrowing_fee_exponent_for_short: Factor,
    pub(super) borrowing_fee_optimal_usage_factor_for_long: Factor,
    pub(super) borrowing_fee_optimal_usage_factor_for_short: Factor,
    pub(super) borrowing_fee_base_factor_for_long: Factor,
    pub(super) borrowing_fee_base_factor_for_short: Factor,
    pub(super) borrowing_fee_above_optimal_usage_factor_for_long: Factor,
    pub(super) borrowing_fee_above_optimal_usage_factor_for_short: Factor,
    // Funding fee.
    pub(super) funding_fee_exponent: Factor,
    pub(super) funding_fee_factor: Factor,
    pub(super) funding_fee_max_factor_per_second: Factor,
    pub(super) funding_fee_min_factor_per_second: Factor,
    pub(super) funding_fee_increase_factor_per_second: Factor,
    pub(super) funding_fee_decrease_factor_per_second: Factor,
    pub(super) funding_fee_threshold_for_stable_funding: Factor,
    pub(super) funding_fee_threshold_for_decrease_funding: Factor,
    // Reserve factor.
    pub(super) reserve_factor: Factor,
    pub(super) open_interest_reserve_factor: Factor,
    // Max pnl factors.
    pub(super) max_pnl_factor_for_long_deposit: Factor,
    pub(super) max_pnl_factor_for_short_deposit: Factor,
    pub(super) max_pnl_factor_for_long_withdrawal: Factor,
    pub(super) max_pnl_factor_for_short_withdrawal: Factor,
    pub(super) max_pnl_factor_for_long_trader: Factor,
    pub(super) max_pnl_factor_for_short_trader: Factor,
    pub(super) max_pnl_factor_for_long_adl: Factor,
    pub(super) max_pnl_factor_for_short_adl: Factor,
    pub(super) min_pnl_factor_after_long_adl: Factor,
    pub(super) min_pnl_factor_after_short_adl: Factor,
    // Other boundary.
    pub(super) max_pool_amount_for_long_token: Factor,
    pub(super) max_pool_amount_for_short_token: Factor,
    pub(super) max_pool_value_for_deposit_for_long_token: Factor,
    pub(super) max_pool_value_for_deposit_for_short_token: Factor,
    pub(super) max_open_interest_for_long: Factor,
    pub(super) max_open_interest_for_short: Factor,
    pub(super) min_tokens_for_first_deposit: Factor,
    reserved: [Factor; 32],
}

impl MarketConfig {
    pub(super) fn init(&mut self) {
        self.swap_impact_exponent = constants::DEFAULT_SWAP_IMPACT_EXPONENT;
        self.swap_impact_positive_factor = constants::DEFAULT_SWAP_IMPACT_POSITIVE_FACTOR;
        self.swap_impact_positive_factor = constants::DEFAULT_SWAP_IMPACT_NEGATIVE_FACTOR;

        self.swap_fee_receiver_factor = constants::DEFAULT_RECEIVER_FACTOR;
        self.swap_fee_factor_for_positive_impact =
            constants::DEFAULT_SWAP_FEE_FACTOR_FOR_POSITIVE_IMPACT;
        self.swap_fee_factor_for_negative_impact =
            constants::DEFAULT_SWAP_FEE_FACTOR_FOR_NEGATIVE_IMPACT;

        self.min_position_size_usd = constants::DEFAULT_MIN_POSITION_SIZE_USD;
        self.min_collateral_value = constants::DEFAULT_MIN_COLLATERAL_VALUE;
        self.min_collateral_factor = constants::DEFAULT_MIN_COLLATERAL_FACTOR;
        self.min_collateral_factor_for_open_interest_multiplier_for_long =
            constants::DEFAULT_MIN_COLLATERAL_FACTOR_FOR_OPEN_INTEREST_FOR_LONG;
        self.min_collateral_factor_for_open_interest_multiplier_for_short =
            constants::DEFAULT_MIN_COLLATERAL_FACTOR_FOR_OPEN_INTEREST_FOR_SHORT;
        self.max_positive_position_impact_factor =
            constants::DEFAULT_MAX_POSITIVE_POSITION_IMPACT_FACTOR;
        self.max_negative_position_impact_factor =
            constants::DEFAULT_MAX_NEGATIVE_POSITION_IMPACT_FACTOR;
        self.max_position_impact_factor_for_liquidations =
            constants::DEFAULT_MAX_POSITION_IMPACT_FACTOR_FOR_LIQUIDATIONS;

        self.position_impact_exponent = constants::DEFAULT_POSITION_IMPACT_EXPONENT;
        self.position_impact_positive_factor = constants::DEFAULT_POSITION_IMPACT_POSITIVE_FACTOR;
        self.position_impact_negative_factor = constants::DEFAULT_POSITION_IMPACT_NEGATIVE_FACTOR;

        self.order_fee_receiver_factor = constants::DEFAULT_RECEIVER_FACTOR;
        self.order_fee_factor_for_positive_impact =
            constants::DEFAULT_ORDER_FEE_FACTOR_FOR_POSITIVE_IMPACT;
        self.order_fee_factor_for_negative_impact =
            constants::DEFAULT_ORDER_FEE_FACTOR_FOR_NEGATIVE_IMPACT;

        self.liquidation_fee_receiver_factor = constants::DEFAULT_RECEIVER_FACTOR;
        self.liquidation_fee_factor = constants::DEFAULT_LIQUIDATION_FEE_FACTOR;

        self.position_impact_distribute_factor =
            constants::DEFAULT_POSITION_IMPACT_DISTRIBUTE_FACTOR;
        self.min_position_impact_pool_amount = constants::DEFAULT_MIN_POSITION_IMPACT_POOL_AMOUNT;

        self.borrowing_fee_receiver_factor = constants::DEFAULT_RECEIVER_FACTOR;
        self.borrowing_fee_factor_for_long = constants::DEFAULT_BORROWING_FEE_FACTOR_FOR_LONG;
        self.borrowing_fee_factor_for_short = constants::DEFAULT_BORROWING_FEE_FACTOR_FOR_SHORT;
        self.borrowing_fee_exponent_for_long = constants::DEFAULT_BORROWING_FEE_EXPONENT_FOR_LONG;
        self.borrowing_fee_exponent_for_short = constants::DEFAULT_BORROWING_FEE_EXPONENT_FOR_SHORT;
        self.borrowing_fee_optimal_usage_factor_for_long =
            constants::DEFAULT_BORROWING_FEE_OPTIMAL_USAGE_FACTOR_FOR_LONG;
        self.borrowing_fee_optimal_usage_factor_for_short =
            constants::DEFAULT_BORROWING_FEE_OPTIMAL_USAGE_FACTOR_FOR_SHORT;
        self.borrowing_fee_base_factor_for_long =
            constants::DEFAULT_BORROWING_FEE_BASE_FACTOR_FOR_LONG;
        self.borrowing_fee_base_factor_for_short =
            constants::DEFAULT_BORROWING_FEE_BASE_FACTOR_FOR_SHORT;
        self.borrowing_fee_above_optimal_usage_factor_for_long =
            constants::DEFAULT_BORROWING_FEE_ABOVE_OPTIMAL_USAGE_FACTOR_FOR_LONG;
        self.borrowing_fee_above_optimal_usage_factor_for_short =
            constants::DEFAULT_BORROWING_FEE_ABOVE_OPTIMAL_USAGE_FACTOR_FOR_SHORT;

        self.funding_fee_exponent = constants::DEFAULT_FUNDING_FEE_EXPONENT;
        self.funding_fee_factor = constants::DEFAULT_FUNDING_FEE_FACTOR;
        self.funding_fee_max_factor_per_second =
            constants::DEFAULT_FUNDING_FEE_MAX_FACTOR_PER_SECOND;
        self.funding_fee_min_factor_per_second =
            constants::DEFAULT_FUNDING_FEE_MIN_FACTOR_PER_SECOND;
        self.funding_fee_increase_factor_per_second =
            constants::DEFAULT_FUNDING_FEE_INCREASE_FACTOR_PER_SECOND;
        self.funding_fee_decrease_factor_per_second =
            constants::DEFAULT_FUNDING_FEE_DECREASE_FACTOR_PER_SECOND;
        self.funding_fee_threshold_for_stable_funding =
            constants::DEFAULT_FUNDING_FEE_THRESHOLD_FOR_STABLE_FUNDING;
        self.funding_fee_threshold_for_decrease_funding =
            constants::DEFAULT_FUNDING_FEE_THRESHOLD_FOR_DECREASE_FUNDING;

        self.reserve_factor = constants::DEFAULT_RECEIVER_FACTOR;
        self.open_interest_reserve_factor = constants::DEFAULT_OPEN_INTEREST_RESERVE_FACTOR;

        self.max_pnl_factor_for_long_deposit = constants::DEFAULT_MAX_PNL_FACTOR_FOR_LONG_DEPOSIT;
        self.max_pnl_factor_for_short_deposit = constants::DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_DEPOSIT;
        self.max_pnl_factor_for_long_withdrawal =
            constants::DEFAULT_MAX_PNL_FACTOR_FOR_LONG_WITHDRAWAL;
        self.max_pnl_factor_for_short_withdrawal =
            constants::DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_WITHDRAWAL;
        self.max_pnl_factor_for_long_trader = constants::DEFAULT_MAX_PNL_FACTOR_FOR_LONG_TRADER;
        self.max_pnl_factor_for_short_trader = constants::DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_TRADER;
        self.max_pnl_factor_for_long_adl = constants::DEFAULT_MAX_PNL_FACTOR_FOR_LONG_ADL;
        self.max_pnl_factor_for_short_adl = constants::DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_ADL;
        self.min_pnl_factor_after_long_adl = constants::DEFAULT_MIN_PNL_FACTOR_AFTER_LONG_ADL;
        self.min_pnl_factor_after_short_adl = constants::DEFAULT_MIN_PNL_FACTOR_AFTER_SHORT_ADL;

        self.max_pool_amount_for_long_token = constants::DEFAULT_MAX_POOL_AMOUNT_FOR_LONG_TOKEN;
        self.max_pool_amount_for_short_token = constants::DEFAULT_MAX_POOL_AMOUNT_FOR_SHORT_TOKEN;

        self.max_pool_value_for_deposit_for_long_token =
            constants::DEFAULT_MAX_POOL_VALUE_FOR_DEPOSIT_LONG_TOKEN;
        self.max_pool_value_for_deposit_for_short_token =
            constants::DEFAULT_MAX_POOL_VALUE_FOR_DEPOSIT_SHORT_TOKEN;

        self.max_open_interest_for_long = constants::DEFAULT_MAX_OPEN_INTEREST_FOR_LONG;
        self.max_open_interest_for_short = constants::DEFAULT_MAX_OPEN_INTEREST_FOR_SHORT;

        self.min_tokens_for_first_deposit = constants::DEFAULT_MIN_TOKENS_FOR_FIRST_DEPOSIT;

        self.set_flag(
            MarketConfigFlag::SkipBorrowingFeeForSmallerSide,
            constants::DEFAULT_SKIP_BORROWING_FEE_FOR_SMALLER_SIDE,
        );
        self.set_flag(
            MarketConfigFlag::IgnoreOpenInterestForUsageFactor,
            constants::DEFAULT_IGNORE_OPEN_INTEREST_FOR_USAGE_FACTOR,
        );
    }

    pub(super) fn get(&self, key: MarketConfigKey) -> &Factor {
        match key {
            MarketConfigKey::SwapImpactExponent => &self.swap_impact_exponent,
            MarketConfigKey::SwapImpactPositiveFactor => &self.swap_impact_positive_factor,
            MarketConfigKey::SwapImpactNegativeFactor => &self.swap_impact_negative_factor,
            MarketConfigKey::SwapFeeReceiverFactor => &self.swap_fee_receiver_factor,
            MarketConfigKey::SwapFeeFactorForPositiveImpact => {
                &self.swap_fee_factor_for_positive_impact
            }
            MarketConfigKey::SwapFeeFactorForNegativeImpact => {
                &self.swap_fee_factor_for_negative_impact
            }
            MarketConfigKey::MinPositionSizeUsd => &self.min_position_size_usd,
            MarketConfigKey::MinCollateralValue => &self.min_collateral_value,
            MarketConfigKey::MinCollateralFactor => &self.min_collateral_factor,
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForLong => {
                &self.min_collateral_factor_for_open_interest_multiplier_for_long
            }
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForShort => {
                &self.min_collateral_factor_for_open_interest_multiplier_for_short
            }
            MarketConfigKey::MaxPositivePositionImpactFactor => {
                &self.max_positive_position_impact_factor
            }
            MarketConfigKey::MaxNegativePositionImpactFactor => {
                &self.max_negative_position_impact_factor
            }
            MarketConfigKey::MaxPositionImpactFactorForLiquidations => {
                &self.max_position_impact_factor_for_liquidations
            }
            MarketConfigKey::PositionImpactExponent => &self.position_impact_exponent,
            MarketConfigKey::PositionImpactPositiveFactor => &self.position_impact_positive_factor,
            MarketConfigKey::PositionImpactNegativeFactor => &self.position_impact_negative_factor,
            MarketConfigKey::OrderFeeReceiverFactor => &self.order_fee_receiver_factor,
            MarketConfigKey::OrderFeeFactorForPositiveImpact => {
                &self.order_fee_factor_for_positive_impact
            }
            MarketConfigKey::OrderFeeFactorForNegativeImpact => {
                &self.order_fee_factor_for_negative_impact
            }
            MarketConfigKey::LiquidationFeeReceiverFactor => &self.liquidation_fee_receiver_factor,
            MarketConfigKey::LiquidationFeeFactor => &self.liquidation_fee_factor,
            MarketConfigKey::PositionImpactDistributeFactor => {
                &self.position_impact_distribute_factor
            }
            MarketConfigKey::MinPositionImpactPoolAmount => &self.min_position_impact_pool_amount,
            MarketConfigKey::BorrowingFeeReceiverFactor => &self.borrowing_fee_receiver_factor,
            MarketConfigKey::BorrowingFeeFactorForLong => &self.borrowing_fee_factor_for_long,
            MarketConfigKey::BorrowingFeeFactorForShort => &self.borrowing_fee_factor_for_short,
            MarketConfigKey::BorrowingFeeExponentForLong => &self.borrowing_fee_exponent_for_long,
            MarketConfigKey::BorrowingFeeExponentForShort => &self.borrowing_fee_exponent_for_short,
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForLong => {
                &self.borrowing_fee_optimal_usage_factor_for_long
            }
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForShort => {
                &self.borrowing_fee_optimal_usage_factor_for_short
            }
            MarketConfigKey::BorrowingFeeBaseFactorForLong => {
                &self.borrowing_fee_base_factor_for_long
            }
            MarketConfigKey::BorrowingFeeBaseFactorForShort => {
                &self.borrowing_fee_base_factor_for_short
            }
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForLong => {
                &self.borrowing_fee_above_optimal_usage_factor_for_long
            }
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForShort => {
                &self.borrowing_fee_above_optimal_usage_factor_for_short
            }
            MarketConfigKey::FundingFeeExponent => &self.funding_fee_exponent,
            MarketConfigKey::FundingFeeFactor => &self.funding_fee_factor,
            MarketConfigKey::FundingFeeMaxFactorPerSecond => {
                &self.funding_fee_max_factor_per_second
            }
            MarketConfigKey::FundingFeeMinFactorPerSecond => {
                &self.funding_fee_min_factor_per_second
            }
            MarketConfigKey::FundingFeeIncreaseFactorPerSecond => {
                &self.funding_fee_increase_factor_per_second
            }
            MarketConfigKey::FundingFeeDecreaseFactorPerSecond => {
                &self.funding_fee_decrease_factor_per_second
            }
            MarketConfigKey::FundingFeeThresholdForStableFunding => {
                &self.funding_fee_threshold_for_stable_funding
            }
            MarketConfigKey::FundingFeeThresholdForDecreaseFunding => {
                &self.funding_fee_threshold_for_decrease_funding
            }
            MarketConfigKey::ReserveFactor => &self.reserve_factor,
            MarketConfigKey::OpenInterestReserveFactor => &self.open_interest_reserve_factor,
            MarketConfigKey::MaxPnlFactorForLongDeposit => &self.max_pnl_factor_for_long_deposit,
            MarketConfigKey::MaxPnlFactorForShortDeposit => &self.max_pnl_factor_for_short_deposit,
            MarketConfigKey::MaxPnlFactorForLongWithdrawal => {
                &self.max_pnl_factor_for_long_withdrawal
            }
            MarketConfigKey::MaxPnlFactorForShortWithdrawal => {
                &self.max_pnl_factor_for_short_withdrawal
            }
            MarketConfigKey::MaxPnlFactorForLongTrader => &self.max_pnl_factor_for_long_trader,
            MarketConfigKey::MaxPnlFactorForShortTrader => &self.max_pnl_factor_for_short_trader,
            MarketConfigKey::MaxPnlFactorForLongAdl => &self.max_pnl_factor_for_long_adl,
            MarketConfigKey::MaxPnlFactorForShortAdl => &self.max_pnl_factor_for_short_adl,
            MarketConfigKey::MinPnlFactorAfterLongAdl => &self.min_pnl_factor_after_long_adl,
            MarketConfigKey::MinPnlFactorAfterShortAdl => &self.min_pnl_factor_after_short_adl,
            MarketConfigKey::MaxPoolAmountForLongToken => &self.max_pool_amount_for_long_token,
            MarketConfigKey::MaxPoolAmountForShortToken => &self.max_pool_amount_for_short_token,
            MarketConfigKey::MaxPoolValueForDepositForLongToken => {
                &self.max_pool_value_for_deposit_for_long_token
            }
            MarketConfigKey::MaxPoolValueForDepositForShortToken => {
                &self.max_pool_value_for_deposit_for_short_token
            }
            MarketConfigKey::MaxOpenInterestForLong => &self.max_open_interest_for_long,
            MarketConfigKey::MaxOpenInterestForShort => &self.max_open_interest_for_short,
            MarketConfigKey::MinTokensForFirstDeposit => &self.min_tokens_for_first_deposit,
        }
    }

    pub(super) fn get_mut(&mut self, key: MarketConfigKey) -> &mut Factor {
        match key {
            MarketConfigKey::SwapImpactExponent => &mut self.swap_impact_exponent,
            MarketConfigKey::SwapImpactPositiveFactor => &mut self.swap_impact_positive_factor,
            MarketConfigKey::SwapImpactNegativeFactor => &mut self.swap_impact_negative_factor,
            MarketConfigKey::SwapFeeReceiverFactor => &mut self.swap_fee_receiver_factor,
            MarketConfigKey::SwapFeeFactorForPositiveImpact => {
                &mut self.swap_fee_factor_for_positive_impact
            }
            MarketConfigKey::SwapFeeFactorForNegativeImpact => {
                &mut self.swap_fee_factor_for_negative_impact
            }
            MarketConfigKey::MinPositionSizeUsd => &mut self.min_position_size_usd,
            MarketConfigKey::MinCollateralValue => &mut self.min_collateral_value,
            MarketConfigKey::MinCollateralFactor => &mut self.min_collateral_factor,
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForLong => {
                &mut self.min_collateral_factor_for_open_interest_multiplier_for_long
            }
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForShort => {
                &mut self.min_collateral_factor_for_open_interest_multiplier_for_short
            }
            MarketConfigKey::MaxPositivePositionImpactFactor => {
                &mut self.max_positive_position_impact_factor
            }
            MarketConfigKey::MaxNegativePositionImpactFactor => {
                &mut self.max_negative_position_impact_factor
            }
            MarketConfigKey::MaxPositionImpactFactorForLiquidations => {
                &mut self.max_position_impact_factor_for_liquidations
            }
            MarketConfigKey::PositionImpactExponent => &mut self.position_impact_exponent,
            MarketConfigKey::PositionImpactPositiveFactor => {
                &mut self.position_impact_positive_factor
            }
            MarketConfigKey::PositionImpactNegativeFactor => {
                &mut self.position_impact_negative_factor
            }
            MarketConfigKey::OrderFeeReceiverFactor => &mut self.order_fee_receiver_factor,
            MarketConfigKey::OrderFeeFactorForPositiveImpact => {
                &mut self.order_fee_factor_for_positive_impact
            }
            MarketConfigKey::OrderFeeFactorForNegativeImpact => {
                &mut self.order_fee_factor_for_negative_impact
            }
            MarketConfigKey::LiquidationFeeReceiverFactor => {
                &mut self.liquidation_fee_receiver_factor
            }
            MarketConfigKey::LiquidationFeeFactor => &mut self.liquidation_fee_factor,
            MarketConfigKey::PositionImpactDistributeFactor => {
                &mut self.position_impact_distribute_factor
            }
            MarketConfigKey::MinPositionImpactPoolAmount => {
                &mut self.min_position_impact_pool_amount
            }
            MarketConfigKey::BorrowingFeeReceiverFactor => &mut self.borrowing_fee_receiver_factor,
            MarketConfigKey::BorrowingFeeFactorForLong => &mut self.borrowing_fee_factor_for_long,
            MarketConfigKey::BorrowingFeeFactorForShort => &mut self.borrowing_fee_factor_for_short,
            MarketConfigKey::BorrowingFeeExponentForLong => {
                &mut self.borrowing_fee_exponent_for_long
            }
            MarketConfigKey::BorrowingFeeExponentForShort => {
                &mut self.borrowing_fee_exponent_for_short
            }
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForLong => {
                &mut self.borrowing_fee_optimal_usage_factor_for_long
            }
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForShort => {
                &mut self.borrowing_fee_optimal_usage_factor_for_short
            }
            MarketConfigKey::BorrowingFeeBaseFactorForLong => {
                &mut self.borrowing_fee_base_factor_for_long
            }
            MarketConfigKey::BorrowingFeeBaseFactorForShort => {
                &mut self.borrowing_fee_base_factor_for_short
            }
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForLong => {
                &mut self.borrowing_fee_above_optimal_usage_factor_for_long
            }
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForShort => {
                &mut self.borrowing_fee_above_optimal_usage_factor_for_short
            }
            MarketConfigKey::FundingFeeExponent => &mut self.funding_fee_exponent,
            MarketConfigKey::FundingFeeFactor => &mut self.funding_fee_factor,
            MarketConfigKey::FundingFeeMaxFactorPerSecond => {
                &mut self.funding_fee_max_factor_per_second
            }
            MarketConfigKey::FundingFeeMinFactorPerSecond => {
                &mut self.funding_fee_min_factor_per_second
            }
            MarketConfigKey::FundingFeeIncreaseFactorPerSecond => {
                &mut self.funding_fee_increase_factor_per_second
            }
            MarketConfigKey::FundingFeeDecreaseFactorPerSecond => {
                &mut self.funding_fee_decrease_factor_per_second
            }
            MarketConfigKey::FundingFeeThresholdForStableFunding => {
                &mut self.funding_fee_threshold_for_stable_funding
            }
            MarketConfigKey::FundingFeeThresholdForDecreaseFunding => {
                &mut self.funding_fee_threshold_for_decrease_funding
            }
            MarketConfigKey::ReserveFactor => &mut self.reserve_factor,
            MarketConfigKey::OpenInterestReserveFactor => &mut self.open_interest_reserve_factor,
            MarketConfigKey::MaxPnlFactorForLongDeposit => {
                &mut self.max_pnl_factor_for_long_deposit
            }
            MarketConfigKey::MaxPnlFactorForShortDeposit => {
                &mut self.max_pnl_factor_for_short_deposit
            }
            MarketConfigKey::MaxPnlFactorForLongWithdrawal => {
                &mut self.max_pnl_factor_for_long_withdrawal
            }
            MarketConfigKey::MaxPnlFactorForShortWithdrawal => {
                &mut self.max_pnl_factor_for_short_withdrawal
            }
            MarketConfigKey::MaxPnlFactorForLongTrader => &mut self.max_pnl_factor_for_long_trader,
            MarketConfigKey::MaxPnlFactorForShortTrader => {
                &mut self.max_pnl_factor_for_short_trader
            }
            MarketConfigKey::MaxPnlFactorForLongAdl => &mut self.max_pnl_factor_for_long_adl,
            MarketConfigKey::MaxPnlFactorForShortAdl => &mut self.max_pnl_factor_for_short_adl,
            MarketConfigKey::MinPnlFactorAfterLongAdl => &mut self.min_pnl_factor_after_long_adl,
            MarketConfigKey::MinPnlFactorAfterShortAdl => &mut self.min_pnl_factor_after_short_adl,
            MarketConfigKey::MaxPoolAmountForLongToken => &mut self.max_pool_amount_for_long_token,
            MarketConfigKey::MaxPoolAmountForShortToken => {
                &mut self.max_pool_amount_for_short_token
            }
            MarketConfigKey::MaxPoolValueForDepositForLongToken => {
                &mut self.max_pool_value_for_deposit_for_long_token
            }
            MarketConfigKey::MaxPoolValueForDepositForShortToken => {
                &mut self.max_pool_value_for_deposit_for_short_token
            }
            MarketConfigKey::MaxOpenInterestForLong => &mut self.max_open_interest_for_long,
            MarketConfigKey::MaxOpenInterestForShort => &mut self.max_open_interest_for_short,
            MarketConfigKey::MinTokensForFirstDeposit => &mut self.min_tokens_for_first_deposit,
        }
    }

    /// Get config flag.
    pub(crate) fn flag(&self, flag: MarketConfigFlag) -> bool {
        let bitmap = MarketConfigFlagBitmap::from_value(self.flag);
        bitmap.get(usize::from(flag as u8))
    }

    /// Set config flag.
    ///
    /// Return the previous value.
    pub(crate) fn set_flag(&mut self, flag: MarketConfigFlag, value: bool) -> bool {
        let mut bitmap = MarketConfigFlagBitmap::from_value(self.flag);
        let previous = bitmap.set(usize::from(flag as u8), value);
        self.flag = bitmap.into_value();
        previous
    }
}

/// Market Config Flags.
#[derive(
    strum::EnumString,
    strum::Display,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
#[repr(u8)]
pub enum MarketConfigFlag {
    /// Skip borrowing fee for smaller side.
    SkipBorrowingFeeForSmallerSide,
    /// Ignore open interest for usage factor.
    IgnoreOpenInterestForUsageFactor,
    // CHECK: cannot have more than `MAX_CONFIG_FLAGS` flags.
}

/// Market config keys.
#[derive(
    strum::EnumString,
    strum::Display,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
#[repr(u16)]
pub enum MarketConfigKey {
    /// Swap impact exponent.
    SwapImpactExponent,
    /// Swap impact positive factor.
    SwapImpactPositiveFactor,
    /// Swap impact negative factor.
    SwapImpactNegativeFactor,
    /// Swap fee receiver factor.
    SwapFeeReceiverFactor,
    /// Swap fee factor for positive impact.
    SwapFeeFactorForPositiveImpact,
    /// Swap fee factor for negative impact.
    SwapFeeFactorForNegativeImpact,
    /// Min position size usd.
    MinPositionSizeUsd,
    /// Min collateral value.
    MinCollateralValue,
    /// Min collateral factor.
    MinCollateralFactor,
    /// Min collateral factor for open interest multiplier for long.
    MinCollateralFactorForOpenInterestMultiplierForLong,
    /// Min collateral factor for open interest multiplier for short.
    MinCollateralFactorForOpenInterestMultiplierForShort,
    /// Max positive position impact factor.
    MaxPositivePositionImpactFactor,
    /// Max negative position impact factor.
    MaxNegativePositionImpactFactor,
    /// Max position impact factor for liquidations.
    MaxPositionImpactFactorForLiquidations,
    /// Position impact exponent.
    PositionImpactExponent,
    /// Position impact positive factor.
    PositionImpactPositiveFactor,
    /// Position impact negative factor.
    PositionImpactNegativeFactor,
    /// Order fee receiver factor.
    OrderFeeReceiverFactor,
    /// Order fee factor for positive impact.
    OrderFeeFactorForPositiveImpact,
    /// Order fee factor for negative impact.
    OrderFeeFactorForNegativeImpact,
    /// Liquidation fee receiver factor.
    LiquidationFeeReceiverFactor,
    /// Liquidation fee factor.
    LiquidationFeeFactor,
    /// Position impact distribute factor.
    PositionImpactDistributeFactor,
    /// Min position impact pool amount.
    MinPositionImpactPoolAmount,
    /// Borrowing fee receiver factor.
    BorrowingFeeReceiverFactor,
    /// Borrowing fee factor for long.
    BorrowingFeeFactorForLong,
    /// Borrowing fee factor for short.
    BorrowingFeeFactorForShort,
    /// Borrowing fee exponent for long.
    BorrowingFeeExponentForLong,
    /// Borrowing fee exponent for short.
    BorrowingFeeExponentForShort,
    /// Borrowing fee optimal usage factor for long.
    BorrowingFeeOptimalUsageFactorForLong,
    /// Borrowing fee optimal usage factor for short.
    BorrowingFeeOptimalUsageFactorForShort,
    /// Borrowing fee base factor for long.
    BorrowingFeeBaseFactorForLong,
    /// Borrowing fee base factor for short.
    BorrowingFeeBaseFactorForShort,
    /// Borrowing fee above optimal usage factor for long.
    BorrowingFeeAboveOptimalUsageFactorForLong,
    /// Borrowing fee above optimal usage factor for short.
    BorrowingFeeAboveOptimalUsageFactorForShort,
    /// Funding fee exponent.
    FundingFeeExponent,
    /// Funding fee factor.
    FundingFeeFactor,
    /// Funding fee max factor per second.
    FundingFeeMaxFactorPerSecond,
    /// Funding fee min factor per second.
    FundingFeeMinFactorPerSecond,
    /// Funding fee increase factor per second.
    FundingFeeIncreaseFactorPerSecond,
    /// Funding fee decrease factor per second.
    FundingFeeDecreaseFactorPerSecond,
    /// Funding fee threshold for stable funding.
    FundingFeeThresholdForStableFunding,
    /// Funding fee threshold for decrease funding.
    FundingFeeThresholdForDecreaseFunding,
    /// Reserve factor.
    ReserveFactor,
    /// Open interest reserve factor.
    OpenInterestReserveFactor,
    /// Max PNL factor for long deposit.
    MaxPnlFactorForLongDeposit,
    /// Max PNL factor for short deposit.
    MaxPnlFactorForShortDeposit,
    /// Max PNL factor for long withdrawal.
    MaxPnlFactorForLongWithdrawal,
    /// Max PNL factor for short withdrawal.
    MaxPnlFactorForShortWithdrawal,
    /// Max PNL factor for long trader.
    MaxPnlFactorForLongTrader,
    /// Max PNL factor for short trader.
    MaxPnlFactorForShortTrader,
    /// Max PNL factor for long ADL.
    MaxPnlFactorForLongAdl,
    /// Max PNL factor for short ADL.
    MaxPnlFactorForShortAdl,
    /// Min PNL factor after long ADL.
    MinPnlFactorAfterLongAdl,
    /// Min PNL factor after short ADL.
    MinPnlFactorAfterShortAdl,
    /// Max pool amount for long token.
    MaxPoolAmountForLongToken,
    /// Max pool amount for short token.
    MaxPoolAmountForShortToken,
    /// Max pool value for deposit for long token.
    MaxPoolValueForDepositForLongToken,
    /// Max pool value for deposit for short token.
    MaxPoolValueForDepositForShortToken,
    /// Max open interest for long.
    MaxOpenInterestForLong,
    /// Max open interest for short.
    MaxOpenInterestForShort,
    /// Min tokens for first deposit.
    MinTokensForFirstDeposit,
}

/// An entry of the config buffer.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Entry {
    /// Key.
    key: u16,
    /// Value.
    value: u128,
}

impl Entry {
    pub(crate) fn new(key: MarketConfigKey, value: Factor) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    /// Get key.
    pub fn key(&self) -> Result<MarketConfigKey> {
        self.key
            .try_into()
            .map_err(|_| error!(CoreError::InvalidMarketConfigKey))
    }

    /// Get value.
    pub fn value(&self) -> Factor {
        self.value
    }
}

/// An entry of the config buffer.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct EntryArgs {
    /// Key.
    pub key: String,
    /// Value.
    pub value: u128,
}

impl TryFrom<EntryArgs> for Entry {
    type Error = Error;

    fn try_from(EntryArgs { key, value }: EntryArgs) -> Result<Self> {
        Ok(Self::new(
            key.parse()
                .map_err(|_| error!(CoreError::InvalidMarketConfigKey))?,
            value,
        ))
    }
}

/// Market Config Buffer.
#[account]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct MarketConfigBuffer {
    /// Store.
    pub store: Pubkey,
    /// Authority.
    pub authority: Pubkey,
    /// Expiration time.
    pub expiry: i64,
    entries: Vec<Entry>,
}

impl MarketConfigBuffer {
    pub(crate) fn init_space(len: usize) -> usize {
        32 + 32 + 8 + 4 + Entry::INIT_SPACE * len
    }

    pub(crate) fn space_after_push(&self, pushed: usize) -> usize {
        let total = self.entries.len() + pushed;
        Self::init_space(total)
    }

    pub(crate) fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    /// Create an iterator of entries.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Return whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
