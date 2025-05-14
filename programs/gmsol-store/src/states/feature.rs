use anchor_lang::prelude::*;

use crate::CoreError;

use super::order::OrderKind;

type DisabledKey = (DomainDisabledFlag, ActionDisabledFlag);

const MAX_DISABLED_FEATURES: usize = 64;
const DISABLED: u8 = u8::MAX;

/// Disabled Features State.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DisabledFeatures {
    map: DisabledMap,
}

impl DisabledFeatures {
    pub(crate) fn get_disabled(
        &self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
    ) -> Option<bool> {
        self.map
            .get(&(domain, action))
            .map(|value| *value == DISABLED)
    }

    pub(crate) fn set_disabled(
        &mut self,
        domain: DomainDisabledFlag,
        action: ActionDisabledFlag,
        disabled: bool,
    ) {
        let value = if disabled { DISABLED } else { 0 };
        self.map.insert(&(domain, action), value);
    }
}

fn to_key(key: &DisabledKey) -> [u8; 2] {
    [key.0 as u8, key.1 as u8]
}

gmsol_utils::fixed_map!(
    DisabledMap,
    2,
    DisabledKey,
    to_key,
    u8,
    MAX_DISABLED_FEATURES,
    0
);

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
    type Error = anchor_lang::prelude::Error;

    fn try_from(kind: OrderKind) -> Result<Self> {
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
            _ => err!(CoreError::UnknownOrderKind),
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
