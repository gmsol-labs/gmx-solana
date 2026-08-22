use anchor_lang::prelude::*;

/// Max number of order flags.
pub const MAX_ORDER_FLAGS: usize = 8;

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

    /// Is a position order initiated by the position's owner.
    ///
    /// These are the only kinds a builder fee may be attached to: the fee pays
    /// whoever built the order for its owner, so a kind the owner did not
    /// initiate has no builder to pay.
    ///
    /// Deliberately spelled as its own list rather than composed from
    /// [`is_increase_position`](Self::is_increase_position) and
    /// [`is_decrease_position`](Self::is_decrease_position): the latter also
    /// covers [`Liquidation`](Self::Liquidation) and
    /// [`AutoDeleveraging`](Self::AutoDeleveraging), which are keeper-initiated,
    /// so that composition reads correct while admitting exactly the two kinds
    /// this predicate exists to exclude.
    pub fn is_user_initiated_position(&self) -> bool {
        matches!(
            self,
            Self::MarketIncrease
                | Self::LimitIncrease
                | Self::MarketDecrease
                | Self::LimitDecrease
                | Self::StopLossDecrease
        )
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

/// Trade Data Flags.
#[allow(clippy::enum_variant_names)]
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum TradeFlag {
    /// Is long.
    IsLong,
    /// Is collateral long.
    IsCollateralLong,
    /// Is increase.
    IsIncrease,
    // CHECK: cannot have more than `8` flags.
}

crate::flags!(TradeFlag, 8, u8);

/// Order Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum OrderFlag {
    /// Whether to keep position account when empty.
    ShouldKeepPositionAccount,
    // CHECK: should have no more than `MAX_ORDER_FLAGS` of flags.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_initiated_position_excludes_passive_kinds() {
        for kind in [OrderKind::Liquidation, OrderKind::AutoDeleveraging] {
            assert!(
                !kind.is_user_initiated_position(),
                "{kind} must not be treated as user-initiated"
            );
            // The guard this predicate exists to replace: the obvious composition
            // accepts both passive kinds, so a regression here is silent.
            assert!(kind.is_increase_position() || kind.is_decrease_position());
        }
    }

    #[test]
    fn user_initiated_position_accepts_owner_kinds() {
        for kind in [
            OrderKind::MarketIncrease,
            OrderKind::LimitIncrease,
            OrderKind::MarketDecrease,
            OrderKind::LimitDecrease,
            OrderKind::StopLossDecrease,
        ] {
            assert!(
                kind.is_user_initiated_position(),
                "{kind} must be treated as user-initiated"
            );
        }
    }

    #[test]
    fn user_initiated_position_excludes_swaps() {
        for kind in [OrderKind::MarketSwap, OrderKind::LimitSwap] {
            assert!(!kind.is_user_initiated_position());
        }
    }
}
