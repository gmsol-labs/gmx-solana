#[cfg(feature = "store")]
anchor_lang::declare_program!(gmsol_store);
#[cfg(feature = "treasury")]
anchor_lang::declare_program!(gmsol_treasury);
#[cfg(feature = "timelock")]
anchor_lang::declare_program!(gmsol_timelock);

/// Constants.
pub mod constants;

/// Model support.
#[cfg(feature = "model")]
pub mod model;

pub use anchor_lang;
pub use bytemuck;
