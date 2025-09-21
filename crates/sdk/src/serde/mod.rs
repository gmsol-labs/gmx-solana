/// Pubkey serialization.
pub mod string_pubkey;

/// Market serialization.
pub mod serde_market;

/// Position serialization.
pub mod serde_position;

/// Token map serialization.
pub mod serde_token_map;

/// GLV serialization.
pub mod serde_glv;

/// Price feed serialization.
pub mod serde_price_feed;

/// Treasury serialization.
#[cfg(feature = "treasury")]
pub mod treasury;

/// LP staking position serialization.
#[cfg(liquidity_provider)]
pub mod serde_lp_position;

/// LP controller serialization.
#[cfg(liquidity_provider)]
pub mod serde_lp_controller;

/// LP global state serialization.
#[cfg(liquidity_provider)]
pub mod serde_lp_global_state;

#[cfg(serde)]
pub use string_pubkey::pubkey;
pub use string_pubkey::StringPubkey;
