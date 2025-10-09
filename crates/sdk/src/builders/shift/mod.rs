/// Builder for the `create_shift` instruction.
pub mod create;

/// Min execution lamports for shift.
pub const MIN_EXECUTION_LAMPORTS_FOR_SHIFT: u64 = 200_000;

pub use create::CreateShift;
