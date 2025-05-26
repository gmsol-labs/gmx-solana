use std::collections::BTreeSet;

use anchor_lang::prelude::{
    borsh::{BorshDeserialize, BorshSerialize},
    *,
};

/// Max number of config flags.
pub const MAX_MARKET_CONFIG_FLAGS: usize = 128;

/// Max number of market flags.
pub const MAX_MARKET_FLAGS: usize = 8;

/// Market error.
#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    /// Not a collateral token.
    #[error("not a collateral token")]
    NotACollateralToken,
}

type MarketResult<T> = std::result::Result<T, MarketError>;

/// Market Metadata.
#[zero_copy]
#[derive(BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MarketMeta {
    /// Market token.
    pub market_token_mint: Pubkey,
    /// Index token.
    pub index_token_mint: Pubkey,
    /// Long token.
    pub long_token_mint: Pubkey,
    /// Short token.
    pub short_token_mint: Pubkey,
}

impl MarketMeta {
    /// Check if the given token is a valid collateral token.
    #[inline]
    pub fn is_collateral_token(&self, token: &Pubkey) -> bool {
        *token == self.long_token_mint || *token == self.short_token_mint
    }

    /// Get pnl token.
    pub fn pnl_token(&self, is_long: bool) -> Pubkey {
        if is_long {
            self.long_token_mint
        } else {
            self.short_token_mint
        }
    }

    /// Check if the given token is long token or short token, and return it's side.
    pub fn to_token_side(&self, token: &Pubkey) -> MarketResult<bool> {
        if *token == self.long_token_mint {
            Ok(true)
        } else if *token == self.short_token_mint {
            Ok(false)
        } else {
            Err(MarketError::NotACollateralToken)
        }
    }

    /// Get opposite token.
    pub fn opposite_token(&self, token: &Pubkey) -> MarketResult<&Pubkey> {
        if *token == self.long_token_mint {
            Ok(&self.short_token_mint)
        } else if *token == self.short_token_mint {
            Ok(&self.long_token_mint)
        } else {
            Err(MarketError::NotACollateralToken)
        }
    }

    /// Get ordered token set.
    pub fn ordered_tokens(&self) -> BTreeSet<Pubkey> {
        BTreeSet::from([
            self.index_token_mint,
            self.long_token_mint,
            self.short_token_mint,
        ])
    }
}

/// Type that has market meta.
pub trait HasMarketMeta {
    fn market_meta(&self) -> &MarketMeta;

    fn is_pure(&self) -> bool {
        let meta = self.market_meta();
        meta.long_token_mint == meta.short_token_mint
    }
}

impl HasMarketMeta for MarketMeta {
    fn market_meta(&self) -> &MarketMeta {
        self
    }
}

/// Get related tokens from markets in order.
pub fn ordered_tokens(from: &impl HasMarketMeta, to: &impl HasMarketMeta) -> BTreeSet<Pubkey> {
    let mut tokens = BTreeSet::default();

    let from = from.market_meta();
    let to = to.market_meta();

    for mint in [
        &from.index_token_mint,
        &from.long_token_mint,
        &from.short_token_mint,
    ]
    .iter()
    .chain(&[
        &to.index_token_mint,
        &to.long_token_mint,
        &to.short_token_mint,
    ]) {
        tokens.insert(**mint);
    }
    tokens
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

/// Market Flags.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum MarketFlag {
    /// Is enabled.
    Enabled,
    /// Is Pure.
    Pure,
    /// Is auto-deleveraging enabled for long.
    AutoDeleveragingEnabledForLong,
    /// Is auto-deleveraging enabled for short.
    AutoDeleveragingEnabledForShort,
    /// Is GT minting enabled.
    GTEnabled,
    // CHECK: cannot have more than `MAX_MARKET_FLAGS` flags.
}
