#![deny(missing_docs)]
#![deny(unreachable_pub)]

//! # GMSOL Solana Utils

/// Error type.
pub mod error;

/// Cluster.
pub mod cluster;

/// Signer.
pub mod signer;

/// Instruction Group.
pub mod instruction_group;

/// Transaction Group.
pub mod transaction_group;

/// Address Lookup Table.
pub mod address_lookup_table;

/// Program.
pub mod program;

/// Compute budget.
pub mod compute_budget;

/// Transaction builder.
pub mod transaction_builder;

/// Transaction bundle builder.
#[cfg(client)]
pub mod bundle_builder;

/// Bundler for bundle builders.
#[cfg(feature = "make-bundle-builder")]
pub mod make_bundle_builder;

/// RPC client extension.
#[cfg(client)]
pub mod client;

/// Utils.
pub mod utils;

pub use crate::{
    error::Error,
    instruction_group::{AtomicGroup, IntoAtomicGroup, ParallelGroup},
    transaction_group::TransactionGroup,
};

/// Result type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(anchor)]
pub use anchor_lang;
#[cfg(client)]
pub use solana_client;
pub use solana_sdk;
