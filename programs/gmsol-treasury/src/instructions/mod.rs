/// Instructions for [`Config`](crate::states::Config).
pub mod config;

/// Instructions for [`TreasuryConfig`](crate::states::TreasuryConfig).
pub mod treasury;

/// Instructions for interacting with the store program.
pub mod store;

/// Instructions for GT bank.
pub mod gt_bank;

pub use config::*;
pub use gt_bank::*;
pub use store::*;
pub use treasury::*;
