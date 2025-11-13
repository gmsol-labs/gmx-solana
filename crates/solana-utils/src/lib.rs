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

/// Program trait.
pub mod program_trait;

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

/// Client traits.
#[cfg(client_traits)]
pub mod client_traits;

/// Utils.
pub mod utils;

/// Jito Group & sender
#[cfg(feature = "jito")]
pub mod jito_group;

pub use crate::{
    error::Error,
    instruction_group::{AtomicGroup, IntoAtomicGroup, ParallelGroup},
    program_trait::{InstructionBuilder, Program, ProgramExt},
    transaction_group::TransactionGroup,
};

/// Result type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(anchor)]
pub use anchor_lang;
#[cfg(client)]
pub use solana_client;
pub use solana_sdk;

#[cfg(feature = "solana-rpc-client-api")]
pub use solana_rpc_client_api;

#[cfg(feature = "solana-account-decoder-client-types")]
pub use solana_account_decoder_client_types;
