#![cfg_attr(docsrs, feature(doc_auto_cfg))]

/// Error type.
pub mod error;

/// Constants.
pub mod constants;

/// Functions for constructing Program Derived Addresses.
pub mod pda;

/// Instruction Builders.
pub mod builders;

/// Market calculations.
pub mod market;

/// Position calculations.
pub mod position;

/// Utilities for serialization.
pub mod serde;

/// Utils.
pub mod utils;

/// JavaScript support.
#[cfg(feature = "js")]
pub mod js;

/// Maintains a graph structured with [`MarketModel`](crate::model::MarketModel) as edges.
#[cfg(feature = "market-graph")]
pub mod market_graph;

/// Client-style API, ported from the `gmsol` crate.
#[cfg(feature = "client")]
pub mod client;

/// Squads support.
#[cfg(feature = "squads")]
pub mod squads;

/// GMSOL resource discovery.
#[cfg(feature = "discover")]
pub mod discover;

/// Model support.
pub mod model {
    pub use gmsol_model::*;
    pub use gmsol_programs::model::*;
}

pub use gmsol_utils as core;

#[cfg(test)]
pub(crate) mod test;

#[cfg(feature = "client")]
pub use client::{ops, Client};

pub use error::Error;

/// Result type.
pub type Result<T> = std::result::Result<T, Error>;

pub use gmsol_programs as programs;
pub use gmsol_solana_utils as solana_utils;

pub use gmsol_solana_utils::{
    AtomicGroup, Error as SolanaUtilsError, IntoAtomicGroup, ParallelGroup, TransactionGroup,
};

#[cfg(feature = "decode")]
pub use gmsol_decode as decode;
