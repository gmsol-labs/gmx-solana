/// Client.
pub mod client;

/// Pull Oracle.
pub mod pull_oracle_impl;

mod access_controller_address;

pub use client::{ApiReport, Client, Credential};
pub use pull_oracle_impl::{filter_feed_ids, ChainlinkPullOracle, ChainlinkPullOracleFactory};

pub type FeedId = [u8; 32];

/// Parse Feed ID.
pub fn parse_feed_id(feed_id: &str) -> crate::Result<FeedId> {
    let feed_id = feed_id.strip_prefix("0x").unwrap_or(feed_id);

    let mut bytes = [0; 32];
    hex::decode_to_slice(feed_id, &mut bytes).map_err(crate::Error::unknown)?;

    Ok(bytes)
}
