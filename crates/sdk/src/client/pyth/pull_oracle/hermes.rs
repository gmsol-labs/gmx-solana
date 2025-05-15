use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use eventsource_stream::Eventsource;
use futures_util::{Stream, TryStreamExt};
use gmsol_utils::{
    market::HasMarketMeta,
    oracle::{pyth_price_with_confidence_to_price, PriceProviderKind},
    token_config::TokenMapAccess,
};
use reqwest::{Client, IntoUrl, Url};

pub use pyth_sdk::Identifier;

use crate::client::pyth::pubkey_to_identifier;

/// Default base URL for Hermes.
pub const DEFAULT_HERMES_BASE: &str = "https://hermes.pyth.network";

/// The SSE endpoint of price updates stream.
pub const PRICE_STREAM: &str = "/v2/updates/price/stream";

/// The endpoint of latest price update.
pub const PRICE_LATEST: &str = "/v2/updates/price/latest";

/// Hermes Client.
#[derive(Debug, Clone)]
pub struct Hermes {
    base: Url,
    client: Client,
}

impl Hermes {
    /// Create a new hermes client with the given base URL.
    pub fn try_new(base: impl IntoUrl) -> crate::Result<Self> {
        Ok(Self {
            base: base.into_url()?,
            client: Client::new(),
        })
    }

    /// Get a stream of price updates.
    pub async fn price_updates(
        &self,
        feed_ids: impl IntoIterator<Item = &Identifier>,
        encoding: Option<EncodingType>,
    ) -> crate::Result<impl Stream<Item = crate::Result<PriceUpdate>> + 'static> {
        let params = get_query(feed_ids, encoding);
        let stream = self
            .client
            .get(self.base.join(PRICE_STREAM).map_err(crate::Error::custom)?)
            .query(&params)
            .send()
            .await?
            .bytes_stream()
            .eventsource()
            .map_err(crate::Error::custom)
            .try_filter_map(|event| {
                let update = deserialize_price_update_event(&event)
                    .inspect_err(
                        |err| tracing::warn!(%err, ?event, "deserialize price update error"),
                    )
                    .ok();
                async { Ok(update) }
            });
        Ok(stream)
    }

    /// Get latest price updates.
    pub async fn latest_price_updates(
        &self,
        feed_ids: impl IntoIterator<Item = &Identifier>,
        encoding: Option<EncodingType>,
    ) -> crate::Result<PriceUpdate> {
        let params = get_query(feed_ids, encoding);
        let update = self
            .client
            .get(self.base.join(PRICE_LATEST).map_err(crate::Error::custom)?)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        Ok(update)
    }

    /// Get unit prices for the given market.
    pub async fn unit_prices_for_market(
        &self,
        token_map: &impl TokenMapAccess,
        market: &impl HasMarketMeta,
    ) -> crate::Result<gmsol_model::price::Prices<u128>> {
        let token_configs =
            token_map
                .token_configs_for_market(market)
                .ok_or(crate::Error::custom(
                    "missing configs for the tokens of the market",
                ))?;
        let feeds = token_configs
            .iter()
            .map(|config| {
                config
                    .get_feed(&PriceProviderKind::Pyth)
                    .map(|feed| pubkey_to_identifier(&feed))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::Error::custom)?;
        let update = self
            .latest_price_updates(feeds.iter().collect::<HashSet<_>>(), None)
            .await?;
        let prices = update
            .parsed
            .iter()
            .map(|price| {
                Ok((
                    Identifier::from_hex(price.id()).map_err(crate::Error::custom)?,
                    &price.price,
                ))
            })
            .collect::<crate::Result<HashMap<Identifier, _>>>()?;
        let [index_token_price, long_token_price, short_token_price] = feeds
            .iter()
            .enumerate()
            .map(|(idx, feed)| {
                let config = token_configs[idx];
                let price = prices
                    .get(feed)
                    .ok_or(crate::Error::custom(format!("missing price for {}", feed)))?;
                let price = pyth_price_with_confidence_to_price(
                    price.price,
                    price.conf,
                    price.expo,
                    config,
                )
                .map_err(crate::Error::custom)?;
                Ok(gmsol_model::price::Price {
                    min: price.min.to_unit_price(),
                    max: price.max.to_unit_price(),
                })
            })
            .collect::<crate::Result<Vec<_>>>()?
            .try_into()
            .expect("must success");
        Ok(gmsol_model::price::Prices {
            index_token_price,
            long_token_price,
            short_token_price,
        })
    }
}

impl Default for Hermes {
    fn default() -> Self {
        Self {
            base: DEFAULT_HERMES_BASE.parse().unwrap(),
            client: Default::default(),
        }
    }
}

fn deserialize_price_update_event(event: &eventsource_stream::Event) -> crate::Result<PriceUpdate> {
    Ok(serde_json::from_str(&event.data)?)
}

/// Price Update.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PriceUpdate {
    pub(crate) binary: BinaryPriceUpdate,
    #[serde(default)]
    parsed: Vec<ParsedPriceUpdate>,
}

impl PriceUpdate {
    /// Get the parsed price udpate.
    pub fn parsed(&self) -> &[ParsedPriceUpdate] {
        &self.parsed
    }

    /// Min timestamp.
    pub fn min_timestamp(&self) -> Option<i64> {
        self.parsed
            .iter()
            .map(|update| update.price.publish_time)
            .min()
    }

    /// Get the binary price update.
    pub fn binary(&self) -> &BinaryPriceUpdate {
        &self.binary
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BinaryPriceUpdate {
    pub(crate) encoding: EncodingType,
    pub(crate) data: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
pub enum EncodingType {
    /// Hex.
    #[default]
    #[serde(rename = "hex")]
    Hex,
    /// Base64.
    #[serde(rename = "base64")]
    Base64,
}

impl fmt::Display for EncodingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hex => write!(f, "hex"),
            Self::Base64 => write!(f, "base64"),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ParsedPriceUpdate {
    id: String,
    price: Price,
    ema_price: Price,
    metadata: Metadata,
}

impl ParsedPriceUpdate {
    /// Get the feed id.
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// Get price.
    pub fn price(&self) -> &Price {
        &self.price
    }

    /// Get EMA Price.
    pub fn ema_price(&self) -> &Price {
        &self.ema_price
    }

    /// Get metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Price {
    /// Price.
    #[serde(with = "pyth_sdk::utils::as_string")]
    price: i64,
    /// Confidence.
    #[serde(with = "pyth_sdk::utils::as_string")]
    conf: u64,
    /// Exponent of the price.
    expo: i32,
    /// Publish unix timestamp (secs) of the price.
    publish_time: i64,
}

impl Price {
    /// Get (raw) price.
    pub fn price(&self) -> i64 {
        self.price
    }

    /// Get the confidence of the price.
    pub fn conf(&self) -> u64 {
        self.conf
    }

    /// Get the exponent of the price.
    pub fn expo(&self) -> i32 {
        self.expo
    }

    /// Get the publish time (unix timestamp in secs).
    pub fn publish_time(&self) -> i64 {
        self.publish_time
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    slot: Option<u64>,
    proof_available_time: Option<i64>,
    prev_publish_time: Option<i64>,
}

impl Metadata {
    /// Get slot.
    pub fn slot(&self) -> Option<u64> {
        self.slot
    }

    /// Get proof available time.
    pub fn proof_available_time(&self) -> Option<i64> {
        self.proof_available_time
    }

    /// Get previous publish time.
    pub fn prev_publish_time(&self) -> Option<i64> {
        self.prev_publish_time
    }
}

fn get_query<'a>(
    feed_ids: impl IntoIterator<Item = &'a Identifier>,
    encoding: Option<EncodingType>,
) -> Vec<(&'static str, String)> {
    let encoding = encoding.or(Some(EncodingType::Base64));
    feed_ids
        .into_iter()
        .map(|id| ("ids[]", id.to_hex()))
        .chain(encoding.map(|encoding| ("encoding", encoding.to_string())))
        .collect()
}
