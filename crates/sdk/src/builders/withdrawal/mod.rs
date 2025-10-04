/// Builder for the `create_withdrawal` instruction.
pub mod create;

/// Min execution lamports for withdrawal.
pub const MIN_EXECUTION_LAMPORTS_FOR_WITHDRAWAL: u64 = 200_000;

pub use create::{CreateWithdrawal, CreateWithdrawalHint};
