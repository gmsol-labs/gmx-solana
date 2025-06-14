#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(clippy::too_many_arguments)]

#[cfg(feature = "store")]
anchor_lang::declare_program!(gmsol_store);
#[cfg(feature = "treasury")]
anchor_lang::declare_program!(gmsol_treasury);
#[cfg(feature = "timelock")]
anchor_lang::declare_program!(gmsol_timelock);
#[cfg(feature = "competition")]
anchor_lang::declare_program!(gmsol_competition);

/// Constants.
pub mod constants;

/// Utilities.
#[cfg(feature = "utils")]
pub mod utils;

/// Error.
pub mod error;

/// Implementations of [`gmsol_model`] traits and utilities.
#[cfg(feature = "model")]
pub mod model;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

pub use anchor_lang;
pub use bytemuck;
