/// Instructions for interacting with [`Config`](crate::states::Config) account.
pub mod config;

/// Instructions for interacting with [`ActionStats`](crate::states::ActionStats) account.
pub mod action_stats;

/// Callback for general action.
pub mod action_callback;

pub use action_callback::*;
pub use action_stats::*;
pub use config::*;
