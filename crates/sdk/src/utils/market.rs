use gmsol_programs::constants::{MARKET_DECIMALS, MARKET_TOKEN_DECIMALS};
use gmsol_utils::market::MarketConfigKey;

use crate::core::{market::MarketMeta, token_config::TokenMapAccess};

/// Token decimals for a market.
#[derive(Debug, Clone, Copy)]
pub struct MarketDecimals {
    /// Index token decimals.
    pub index_token_decimals: u8,
    /// Long token decimals.
    pub long_token_decimals: u8,
    /// Short token decimals.
    pub short_token_decimals: u8,
}

impl MarketDecimals {
    /// Create from market meta and token map.
    pub fn new(meta: &MarketMeta, token_map: &impl TokenMapAccess) -> crate::Result<Self> {
        let index_token_decimals = token_map
            .get(&meta.index_token_mint)
            .ok_or(crate::Error::NotFound)?
            .token_decimals;
        let long_token_decimals = token_map
            .get(&meta.long_token_mint)
            .ok_or(crate::Error::NotFound)?
            .token_decimals;
        let short_token_decimals = token_map
            .get(&meta.short_token_mint)
            .ok_or(crate::Error::NotFound)?
            .token_decimals;

        Ok(Self {
            index_token_decimals,
            long_token_decimals,
            short_token_decimals,
        })
    }

    /// Returns the decimals for the given market config key.
    pub fn market_config_decimals(&self, key: MarketConfigKey) -> crate::Result<u8> {
        let decimals = match key {
            MarketConfigKey::SwapImpactExponent => MARKET_DECIMALS,
            MarketConfigKey::SwapImpactPositiveFactor => MARKET_DECIMALS,
            MarketConfigKey::SwapImpactNegativeFactor => MARKET_DECIMALS,
            MarketConfigKey::SwapFeeReceiverFactor => MARKET_DECIMALS,
            MarketConfigKey::SwapFeeFactorForPositiveImpact => MARKET_DECIMALS,
            MarketConfigKey::SwapFeeFactorForNegativeImpact => MARKET_DECIMALS,
            MarketConfigKey::MinPositionSizeUsd => MARKET_DECIMALS,
            MarketConfigKey::MinCollateralValue => MARKET_DECIMALS,
            MarketConfigKey::MinCollateralFactor => MARKET_DECIMALS,
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForLong => MARKET_DECIMALS,
            MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForShort => {
                MARKET_DECIMALS
            }
            MarketConfigKey::MaxPositivePositionImpactFactor => MARKET_DECIMALS,
            MarketConfigKey::MaxNegativePositionImpactFactor => MARKET_DECIMALS,
            MarketConfigKey::MaxPositionImpactFactorForLiquidations => MARKET_DECIMALS,
            MarketConfigKey::PositionImpactExponent => MARKET_DECIMALS,
            MarketConfigKey::PositionImpactPositiveFactor => MARKET_DECIMALS,
            MarketConfigKey::PositionImpactNegativeFactor => MARKET_DECIMALS,
            MarketConfigKey::OrderFeeReceiverFactor => MARKET_DECIMALS,
            MarketConfigKey::OrderFeeFactorForPositiveImpact => MARKET_DECIMALS,
            MarketConfigKey::OrderFeeFactorForNegativeImpact => MARKET_DECIMALS,
            MarketConfigKey::LiquidationFeeReceiverFactor => MARKET_DECIMALS,
            MarketConfigKey::LiquidationFeeFactor => MARKET_DECIMALS,
            MarketConfigKey::PositionImpactDistributeFactor => MARKET_DECIMALS,
            MarketConfigKey::MinPositionImpactPoolAmount => self.index_token_decimals,
            MarketConfigKey::BorrowingFeeReceiverFactor => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeFactorForLong => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeFactorForShort => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeExponentForLong => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeExponentForShort => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForLong => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeOptimalUsageFactorForShort => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeBaseFactorForLong => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeBaseFactorForShort => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForLong => MARKET_DECIMALS,
            MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForShort => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeExponent => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeFactor => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeMaxFactorPerSecond => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeMinFactorPerSecond => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeIncreaseFactorPerSecond => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeDecreaseFactorPerSecond => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeThresholdForStableFunding => MARKET_DECIMALS,
            MarketConfigKey::FundingFeeThresholdForDecreaseFunding => MARKET_DECIMALS,
            MarketConfigKey::ReserveFactor => MARKET_DECIMALS,
            MarketConfigKey::OpenInterestReserveFactor => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForLongDeposit => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForShortDeposit => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForLongWithdrawal => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForShortWithdrawal => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForLongTrader => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForShortTrader => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForLongAdl => MARKET_DECIMALS,
            MarketConfigKey::MaxPnlFactorForShortAdl => MARKET_DECIMALS,
            MarketConfigKey::MinPnlFactorAfterLongAdl => MARKET_DECIMALS,
            MarketConfigKey::MinPnlFactorAfterShortAdl => MARKET_DECIMALS,
            MarketConfigKey::MaxPoolAmountForLongToken => self.long_token_decimals,
            MarketConfigKey::MaxPoolAmountForShortToken => self.short_token_decimals,
            MarketConfigKey::MaxPoolValueForDepositForLongToken => MARKET_DECIMALS,
            MarketConfigKey::MaxPoolValueForDepositForShortToken => MARKET_DECIMALS,
            MarketConfigKey::MaxOpenInterestForLong => MARKET_DECIMALS,
            MarketConfigKey::MaxOpenInterestForShort => MARKET_DECIMALS,
            MarketConfigKey::MinTokensForFirstDeposit => MARKET_TOKEN_DECIMALS,
            key => {
                return Err(crate::Error::custom(format!(
                    "the decimals of `{key}` is unknown"
                )))
            }
        };
        Ok(decimals)
    }
}
