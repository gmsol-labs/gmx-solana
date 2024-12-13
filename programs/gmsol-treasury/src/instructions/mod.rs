/// Instructions for [`Config`](crate::states::Config).
pub mod config;

/// Instructions for [`Treasury`](crate::states::Treasury).
pub mod treasury;

/// Instructions for interacting with the store program.
pub mod store;

pub use config::*;
pub use store::*;
pub use treasury::*;
