/// Instruction builders related to market config management.
mod config;

pub use config::*;
use solana_sdk::pubkey::Pubkey;

/// Builder for market-token-related instructions.
pub trait MarketTokenIxBuilder {
    /// Returns market token.
    fn market_token(&self) -> &Pubkey;
}
