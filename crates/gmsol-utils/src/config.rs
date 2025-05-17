use crate::order::OrderKind;

/// Config error.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Unsupported domain.
    #[error("unsupported domain")]
    UnsupportedDomain,
}

/// Domain Disabled Flag.
#[derive(Clone, Copy, strum::EnumString, strum::Display)]
#[repr(u8)]
#[non_exhaustive]
#[strum(serialize_all = "kebab-case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "kebab-case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum DomainDisabledFlag {
    /// Market Swap Order.
    MarketSwap = 0,
    /// Market Increase Order.
    MarketIncrease = 1,
    /// Market Decrease Order.
    MarketDecrease = 2,
    /// Liquidation Order.
    Liquidation = 3,
    /// Auto-deleveraging Order.
    AutoDeleveraging = 4,
    /// Limit Swap Order.
    LimitSwap = 5,
    /// Limit Increase Order.
    LimitIncrease = 6,
    /// Limit Decrease Order.
    LimitDecrease = 7,
    /// Stop-loss Decrease Order.
    StopLossDecrease = 8,
    /// Deposit.
    Deposit = 9,
    /// Withdrawal.
    Withdrawal = 10,
    /// Shift.
    Shift = 11,
    /// GLV deposit.
    GlvDeposit = 12,
    /// GLV withdrawal.
    GlvWithdrawal = 13,
    /// GLV shift.
    GlvShift = 14,
}

impl TryFrom<OrderKind> for DomainDisabledFlag {
    type Error = ConfigError;

    fn try_from(kind: OrderKind) -> Result<Self, Self::Error> {
        match kind {
            OrderKind::MarketSwap => Ok(Self::MarketSwap),
            OrderKind::MarketIncrease => Ok(Self::MarketIncrease),
            OrderKind::MarketDecrease => Ok(Self::MarketDecrease),
            OrderKind::Liquidation => Ok(Self::Liquidation),
            OrderKind::AutoDeleveraging => Ok(Self::AutoDeleveraging),
            OrderKind::LimitSwap => Ok(Self::LimitSwap),
            OrderKind::LimitIncrease => Ok(Self::LimitIncrease),
            OrderKind::LimitDecrease => Ok(Self::LimitDecrease),
            OrderKind::StopLossDecrease => Ok(Self::StopLossDecrease),
        }
    }
}

/// Action Disabled Flag.
#[derive(Clone, Copy, Default, strum::EnumString, strum::Display)]
#[repr(u8)]
#[non_exhaustive]
#[strum(serialize_all = "kebab-case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "kebab-case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum ActionDisabledFlag {
    /// Default Action.
    #[default]
    Default = 0,
    /// Create.
    Create = 1,
    /// Update.
    Update = 2,
    /// Execute.
    Execute = 3,
    /// Cancel.
    Cancel = 4,
}

/// Display feature.
pub fn display_feature(domain: DomainDisabledFlag, action: ActionDisabledFlag) -> String {
    let action = match action {
        ActionDisabledFlag::Default => String::new(),
        action => format!(":{action}"),
    };
    format!("{domain}{action}")
}

/// Amount keys.
#[derive(strum::EnumString, strum::Display, Clone, Copy)]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
pub enum AmountKey {
    /// Claimable time window (seconds).
    ClaimableTimeWindow,
    /// Recent time window (seconds).
    RecentTimeWindow,
    /// Request expiration (seconds).
    RequestExpiration,
    /// Oracle max age (seconds).
    OracleMaxAge,
    /// Oracle max timestamp range (seconds).
    OracleMaxTimestampRange,
    /// Max timestamp excess for oracle timestamp (seconds).
    OracleMaxFutureTimestampExcess,
    /// Max ADL prices staleness (seconds).
    AdlPricesMaxStaleness,
}

/// Factor keys.
#[derive(strum::EnumString, strum::Display, Clone, Copy)]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
pub enum FactorKey {
    /// Oracle Ref Price Deviation.
    OracleRefPriceDeviation,
    /// Order fee discount for referred user.
    OrderFeeDiscountForReferredUser,
}

/// Address keys.
#[derive(strum::EnumString, strum::Display, Clone, Copy)]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
pub enum AddressKey {
    /// Holding.
    Holding,
}
