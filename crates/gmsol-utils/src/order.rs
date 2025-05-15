use anchor_lang::prelude::*;

/// Order Kind.
#[derive(
    AnchorSerialize,
    AnchorDeserialize,
    Clone,
    InitSpace,
    Copy,
    strum::EnumString,
    strum::Display,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    Debug,
)]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
#[repr(u8)]
pub enum OrderKind {
    /// Liquidation: allows liquidation of positions if the criteria for liquidation are met.
    Liquidation,
    /// Auto-deleveraging Order.
    AutoDeleveraging,
    /// Swap token A to token B at the current market price.
    ///
    /// The order will be cancelled if the `min_output_amount` cannot be fulfilled.
    MarketSwap,
    /// Increase position at the current market price.
    ///
    /// The order will be cancelled if the position cannot be increased at the acceptable price.
    MarketIncrease,
    /// Decrease position at the current market price.
    ///
    /// The order will be cancelled if the position cannot be decreased at the acceptable price.
    MarketDecrease,
    /// Limit Swap.
    LimitSwap,
    /// Limit Increase.
    LimitIncrease,
    /// Limit Decrease.
    LimitDecrease,
    /// Stop-Loss Decrease.
    StopLossDecrease,
}

impl OrderKind {
    /// Is market order.
    pub fn is_market(&self) -> bool {
        matches!(
            self,
            Self::MarketSwap | Self::MarketIncrease | Self::MarketDecrease
        )
    }

    /// Is swap order.
    pub fn is_swap(&self) -> bool {
        matches!(self, Self::MarketSwap | Self::LimitSwap)
    }

    /// Is increase position order.
    pub fn is_increase_position(&self) -> bool {
        matches!(self, Self::LimitIncrease | Self::MarketIncrease)
    }

    /// Is decrease position order.
    pub fn is_decrease_position(&self) -> bool {
        matches!(
            self,
            Self::LimitDecrease
                | Self::MarketDecrease
                | Self::Liquidation
                | Self::AutoDeleveraging
                | Self::StopLossDecrease
        )
    }

    /// Is market decrease.
    pub fn is_market_decrease(&self) -> bool {
        matches!(self, Self::MarketDecrease)
    }
}

/// Order side.
#[derive(
    Clone,
    Copy,
    strum::EnumString,
    strum::Display,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[non_exhaustive]
#[repr(u8)]
pub enum OrderSide {
    /// Long.
    Long,
    /// Short.
    Short,
}

impl OrderSide {
    /// Return whether the side is long.
    pub fn is_long(&self) -> bool {
        matches!(self, Self::Long)
    }
}

/// Position Kind.
#[non_exhaustive]
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum PositionKind {
    /// Uninitialized.
    Uninitialized,
    /// Long position.
    Long,
    /// Short position.
    Short,
}

/// Position Cut Kind.
#[derive(Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub enum PositionCutKind {
    /// Liquidate.
    Liquidate,
    /// AutoDeleverage.
    AutoDeleverage(u128),
}

impl PositionCutKind {
    /// Get size delta.
    pub fn size_delta_usd(&self, size_in_usd: u128) -> u128 {
        match self {
            Self::Liquidate => size_in_usd,
            Self::AutoDeleverage(delta) => size_in_usd.min(*delta),
        }
    }

    /// Convert into [`OrderKind`].
    pub fn to_order_kind(&self) -> OrderKind {
        match self {
            Self::Liquidate => OrderKind::Liquidation,
            Self::AutoDeleverage(_) => OrderKind::AutoDeleveraging,
        }
    }
}
