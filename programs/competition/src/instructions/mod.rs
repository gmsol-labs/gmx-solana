//! Instruction dispatch table for the gmsol‑competition program.

/// Initialize the global [`Competition`](crate::states::Competition) account.
pub mod competition;

/// Lazily create a [`Participant`](crate::states::Participant) PDA.
pub mod participant;

/// Callback entry invoked by the GMX‑Solana store program on each trade.
pub mod trade_callback;

pub use competition::*;
pub use participant::*;
pub use trade_callback::*;
