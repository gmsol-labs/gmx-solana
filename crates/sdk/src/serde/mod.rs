/// Pubkey serialization.
pub mod string_pubkey;

#[cfg(serde)]
pub use string_pubkey::pubkey;
pub use string_pubkey::StringPubkey;
