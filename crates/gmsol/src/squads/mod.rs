//!
//! To avoid dependency conflicts, some parts of the `squads_multisig` SDK are reimplemented
//! with the help of [`declare_program!`](anchor_lang::declare_program!).
//! The implementation is based on: https://crates.io/crates/squads-multisig/2.1.0

mod pda;
mod small_vec;
mod utils;

/// Operations.
pub mod ops;

anchor_lang::declare_program!(squads_multisig_v4);

pub use ops::{SquadsOps, SquadsProposal, SquadsVaultTransaction};
pub use pda::{get_proposal_pda, get_transaction_pda, get_vault_pda};
