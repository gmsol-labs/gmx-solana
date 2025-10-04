/// Builder for the `create_glv_withdrawal` instruction.
pub mod create;

/// Min execution lamports for GLV withdrawal.
pub const MIN_EXECUTION_LAMPORTS_FOR_GLV_WITHDRAWAL: u64 = 200_000;

pub use create::{CreateGlvWithdrawal, CreateGlvWithdrawalHint};
