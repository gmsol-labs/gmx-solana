/// Program Derived Addresses for GMSOL Programs.
pub mod pda;

/// GMSOL Client.
pub mod client;

/// GMSOL resource discovery.
#[cfg(feature = "discover")]
pub mod discover;

/// Error type for `gmsol`.
pub mod error;

/// Instructions for the store program.
pub mod store;

/// Instructions for the exchange funtionality.
pub mod exchange;

/// Instructions for the treasury program.
pub mod treasury;

/// Instructions for the timelock program.
pub mod timelock;

/// Address Lookup Table operations.
pub mod alt;

/// IDL operations.
pub mod idl;

/// Utils.
pub mod utils;

/// GMSOL types.
pub mod types;

/// Program IDs.
pub mod program_ids;

/// GMSOL constants.
pub mod constants {
    pub use gmsol_store::constants::*;
}

/// Switchboard integration.
pub mod switchboard;

/// Chainlink integartion.
pub mod chainlink;

/// Pyth intergration.
pub mod pyth;

#[cfg(feature = "squads")]
/// Squads integation.
pub mod squads;

#[cfg(feature = "cli")]
/// CLI support.
pub mod cli;

#[cfg(feature = "migration")]
/// Migration.
pub mod migration;

/// Test Utils.
#[cfg(test)]
mod test;

pub use client::{Client, ClientOptions};
pub use error::Error;
pub use gmsol_model as model;
pub use gmsol_solana_utils as solana_utils;

#[cfg(feature = "decode")]
pub use gmsol_decode as decode;

pub type Result<T> = std::result::Result<T, Error>;
