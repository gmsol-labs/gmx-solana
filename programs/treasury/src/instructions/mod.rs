/// Instructions for [`Config`](crate::states::Config).
pub mod config;

/// Instructions for [`TreasuryVaultConfig`](crate::states::TreasuryVaultConfig).
pub mod treasury;

/// Instructions for interacting with the store program.
pub mod store;

/// Instructions for GT bank.
pub mod gt_bank;

/// Instructions for swapping funds.
pub mod swap;

pub use config::*;
pub use gt_bank::*;
pub use store::*;
pub use swap::*;
pub use treasury::*;
