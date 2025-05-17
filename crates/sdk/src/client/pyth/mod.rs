/// Pyth pull oracle support.
pub mod pull_oracle;

pub use pull_oracle::{
    hermes::{EncodingType, Hermes},
    PythPullOracle,
};

use pyth_sdk::Identifier;
use solana_sdk::pubkey::Pubkey;

/// Convert a pubkey to feed id.
pub fn pubkey_to_identifier(feed: &Pubkey) -> Identifier {
    Identifier::new(feed.to_bytes())
}
