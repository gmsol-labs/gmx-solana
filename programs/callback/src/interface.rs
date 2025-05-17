use anchor_lang::{prelude::*, CheckId};

pub use crate::{
    cpi::{accounts::Callback, on_closed, on_created, on_executed},
    CALLBACK_AUTHORITY_SEED,
};

/// Callback interface for GMX-Solana.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallbackInterface;

impl CheckId for CallbackInterface {
    fn check_id(_id: &anchor_lang::prelude::Pubkey) -> anchor_lang::Result<()> {
        Ok(())
    }
}

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
