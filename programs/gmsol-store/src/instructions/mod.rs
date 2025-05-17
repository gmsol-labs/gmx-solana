/// Instructions for [`Store`](crate::states::Store) account.
pub mod store;

/// Instructions for config magament.
pub mod config;

/// Instructions for roles management.
pub mod roles;

/// Instructions for [`TokenConfig`](crate::states::TokenConfig) management.
pub mod token_config;

/// Instructions for [`Market`](crate::states::Market) account.
pub mod market;

/// Instructions for tokens and token accounts.
pub mod token;

/// Instructions for [`Oracle`](crate::states::Oracle) account.
pub mod oracle;

/// Instructions for the exchange funtionality.
pub mod exchange;

/// Instructions for GT.
pub mod gt;

/// Instructions for User accounts.
pub mod user;

/// Instructions for disabled features.
pub mod feature;

/// Instructions for GLV.
pub mod glv;

/// Instructions for migrations.
pub mod migration;

/// Instructions for callback.
pub mod callback;

pub use callback::*;
pub use config::*;
pub use exchange::*;
pub use feature::*;
pub use glv::*;
pub use gt::*;
pub use market::*;
pub use migration::*;
pub use oracle::*;
pub use roles::*;
pub use store::*;
pub use token::*;
pub use token_config::*;
pub use user::*;
