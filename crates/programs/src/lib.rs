#[cfg(feature = "store")]
anchor_lang::declare_program!(gmsol_store);
#[cfg(feature = "treasury")]
anchor_lang::declare_program!(gmsol_treasury);

/// Constants.
pub mod constants;

/// Model support.
#[cfg(feature = "model")]
pub mod model;

pub use anchor_lang;
pub use bytemuck;
