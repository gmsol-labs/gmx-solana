/// Builder for the `create_deposit` instruction.
pub mod create;

/// Min execution lamports for deposit.
pub const MIN_EXECUTION_LAMPORTS_FOR_DEPOSIT: u64 = 200_000;

pub use create::{CreateDeposit, CreateDepositHint};
