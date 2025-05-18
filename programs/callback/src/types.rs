use anchor_lang::prelude::*;

/// Action kind.
#[non_exhaustive]
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    PartialEq,
    Eq,
    AnchorSerialize,
    AnchorDeserialize,
    InitSpace,
)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "display", derive(strum::EnumString, strum::Display))]
#[cfg_attr(feature = "display", strum(serialize_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ActionKind {
    /// Deposit.
    Deposit,
    /// Withdrawal.
    Withdrawal,
    /// Shift.
    Shift,
    /// Order.
    Order,
    /// GLV deposit.
    GlvDeposit,
    /// GLV withdrawal.
    GlvWithdrawal,
    /// GLV shift.
    GlvShift,
}
