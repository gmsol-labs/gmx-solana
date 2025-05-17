use anchor_lang::prelude::*;

/// Action error.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// Preconditions are not met.
    #[error("preconditions are not met: {0}")]
    PreconditionsAreNotMet(&'static str),
}

type ActionResult<T> = std::result::Result<T, ActionError>;

/// Max number of aciton flags.
pub const MAX_ACTION_FLAGS: usize = 8;

/// Action Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum ActionFlag {
    /// Should unwrap native token.
    ShouldUnwrapNativeToken,
    // CHECK: should have no more than `MAX_ACTION_FLAGS` of flags.
}

/// Action State.
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
    AnchorSerialize,
    AnchorDeserialize,
    InitSpace,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ActionState {
    /// Pending.
    Pending,
    /// Completed.
    Completed,
    /// Cancelled.
    Cancelled,
}

impl ActionState {
    /// Transition to Completed State.
    pub fn completed(self) -> ActionResult<Self> {
        let Self::Pending = self else {
            return Err(ActionError::PreconditionsAreNotMet("expected pending"));
        };
        Ok(Self::Completed)
    }

    /// Transition to Cancelled State.
    pub fn cancelled(self) -> ActionResult<Self> {
        let Self::Pending = self else {
            return Err(ActionError::PreconditionsAreNotMet("expected pending"));
        };
        Ok(Self::Cancelled)
    }

    /// Check if the state is completed or cancelled.
    pub fn is_completed_or_cancelled(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// Check if the state is pending.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Check if the state is cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Check if the state is completed.
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }
}

/// Callback kind for action.
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
    AnchorSerialize,
    AnchorDeserialize,
    InitSpace,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ActionCallbackKind {
    /// Disabled.
    Disabled,
    /// General.
    General,
}
