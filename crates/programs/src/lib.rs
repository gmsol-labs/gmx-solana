#[cfg(feature = "store")]
anchor_lang::declare_program!(gmsol_store);
#[cfg(feature = "treasury")]
anchor_lang::declare_program!(gmsol_treasury);
#[cfg(feature = "timelock")]
anchor_lang::declare_program!(gmsol_timelock);

/// Constants.
pub mod constants;

/// Utils.
pub mod utils;

/// Error.
pub mod error;

/// Model support.
#[cfg(feature = "model")]
pub mod model;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

pub use anchor_lang;
pub use bytemuck;
