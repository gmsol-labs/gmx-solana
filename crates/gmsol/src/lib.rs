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

/// Chainlink intergartion.
pub mod chainlink;

/// Pyth integration.
pub mod pyth;

/// Test Utils.
#[cfg(test)]
mod test;

pub use client::{Client, ClientOptions};
pub use error::Error;
pub use gmsol_model as model;

#[cfg(feature = "decode")]
pub use gmsol_decode as decode;

pub type Result<T> = std::result::Result<T, Error>;
