use gmsol_utils::{
    oracle::PriceProviderKind,
    token_config::{FeedConfig, TokenConfig},
};
use indexmap::IndexMap;
use strum::IntoEnumIterator;

use crate::utils::Value;

/// Serializable version of [`TokenConfig`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeTokenConfig {
    /// Token name.
    pub name: String,
    /// Indicates whether the token is enabled.
    pub is_enabled: bool,
    /// Indicates whether the token is synthetic.
    pub is_synthetic: bool,
    /// The decimals of token amount.
    pub token_decimals: u8,
    /// The precision of the price.
    pub price_precision: u8,
    /// Expected price provider.
    pub expected_provider: PriceProviderKind,
    /// Feeds.
    pub feeds: IndexMap<PriceProviderKind, SerdeFeedConfig>,
    /// Heartbeat duration.
    pub heartbeat_duration: u32,
}

impl<'a> TryFrom<&'a TokenConfig> for SerdeTokenConfig {
    type Error = crate::Error;

    fn try_from(config: &'a TokenConfig) -> Result<Self, Self::Error> {
        let feeds = PriceProviderKind::iter()
            .filter_map(|kind| {
                config
                    .get_feed_config(&kind)
                    .ok()
                    .map(|config| (kind, SerdeFeedConfig::from_feed_config(kind, config)))
            })
            .collect();
        Ok(Self {
            name: config.name().map_err(crate::Error::custom)?.to_string(),
            is_enabled: config.is_enabled(),
            is_synthetic: config.is_synthetic(),
            token_decimals: config.token_decimals(),
            price_precision: config.precision(),
            expected_provider: config.expected_provider().map_err(crate::Error::custom)?,
            feeds,
            heartbeat_duration: config.heartbeat_duration(),
        })
    }
}

/// Encoding.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(serde, serde(rename_all = "snake_case"))]
pub enum Encoding {
    /// Hex.
    Hex,
    /// Base58,
    Base58,
}

/// Serializable version of [`FeedConfig`]
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeFeedConfig {
    /// Feed ID
    pub feed_id: String,
    /// The encoding type of Feed ID.
    pub feed_id_encoding: Encoding,
    /// Timestamp adjustment.
    pub timestamp_adjustment: u32,
    /// Max deviation factor.
    #[cfg_attr(serde, serde(default))]
    pub max_deviation_factor: Option<Value>,
}

impl SerdeFeedConfig {
    /// Create from [`FeedConfig`].
    pub fn from_feed_config(kind: PriceProviderKind, config: &FeedConfig) -> Self {
        let max_deviation_factor = config.max_deviation_factor().map(Value::from_u128);
        match kind {
            PriceProviderKind::Pyth | PriceProviderKind::ChainlinkDataStreams => Self {
                feed_id_encoding: Encoding::Hex,
                feed_id: format!("0x{}", hex::encode(config.feed())),
                timestamp_adjustment: config.timestamp_adjustment(),
                max_deviation_factor,
            },
            _ => Self {
                feed_id_encoding: Encoding::Base58,
                feed_id: config.feed().to_string(),
                timestamp_adjustment: config.timestamp_adjustment(),
                max_deviation_factor,
            },
        }
    }

    /// Get formatted feed id.
    pub fn formatted_feed_id(&self) -> String {
        match self.feed_id_encoding {
            Encoding::Hex => format!("0x{}", self.feed_id),
            Encoding::Base58 => self.feed_id.clone(),
        }
    }
}
