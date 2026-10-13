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

/// ENV for Pyth Hermes API key.
pub const ENV_API_KEY: &str = "PYTH_API_KEY";

/// The SSE endpoint of price updates stream.
pub const PRICE_STREAM: &str = "/v2/updates/price/stream";

/// The endpoint of latest price update.
pub const PRICE_LATEST: &str = "/v2/updates/price/latest";

/// The endpoint of price update at a specific publish time.
#[cfg(feature = "nightly-pyth-historical-api")]
pub const PRICE_HISTORICAL: &str = "/v2/updates/price/";

/// Hermes Client.
#[derive(Clone)]
pub struct Hermes {
    base: Url,
    api_key: Option<String>,
    client: Client,
}

impl fmt::Debug for Hermes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hermes")
            .field("base", &self.base)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("client", &self.client)
            .finish()
    }
}

/// Normalize a base URL so endpoint paths can be joined onto it without
/// discarding a path prefix.
///
/// `Url::join` follows RFC 3986: joining `/v2/...` onto
/// `https://host/hermes` yields `https://host/v2/...`, dropping `/hermes`.
/// Making the base path end with `/` and joining relative paths keeps it.
fn normalize_base(mut base: Url) -> crate::Result<Url> {
    if base.cannot_be_a_base() {
        return Err(crate::Error::custom(format!(
            "invalid Hermes base URL: {base}"
        )));
    }
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    Ok(base)
}

impl Hermes {
    /// Create a new hermes client with the given base URL.
    ///
    /// A path prefix in the base (e.g. `https://host/hermes`) is preserved.
    pub fn try_new(base: impl IntoUrl) -> crate::Result<Self> {
        Ok(Self {
            base: normalize_base(base.into_url()?)?,
            api_key: None,
            client: Client::new(),
        })
    }

    /// Create a new hermes client with the given base URL and API key.
    ///
    /// A path prefix in the base (e.g. `https://host/hermes`) is preserved.
    pub fn try_new_with_api_key(
        base: impl IntoUrl,
        api_key: impl Into<String>,
    ) -> crate::Result<Self> {
        Ok(Self {
            base: normalize_base(base.into_url()?)?,
            api_key: Some(api_key.into()),
            client: Client::new(),
        })
    }

    /// Resolve an endpoint path against the base URL, keeping any base path
    /// prefix.
    fn endpoint(&self, path: &str) -> crate::Result<Url> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(crate::Error::custom)
    }

    /// Create a new Hermes client from default ENVs.
    ///
    /// Reads [`ENV_API_KEY`] and uses [`DEFAULT_HERMES_BASE`].
    pub fn from_default_envs() -> crate::Result<Self> {
        let api_key = std::env::var(ENV_API_KEY).map_err(crate::Error::custom)?;
        Self::try_new_with_api_key(DEFAULT_HERMES_BASE, api_key)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(api_key) => request.header("Authorization", format!("Bearer {api_key}")),
            None => request,
        }
    }

    /// Get a stream of price updates.
    pub async fn price_updates(
        &self,
        feed_ids: impl IntoIterator<Item = &Identifier>,
        encoding: Option<EncodingType>,
    ) -> crate::Result<impl Stream<Item = crate::Result<PriceUpdate>> + 'static> {
        let params = get_query(feed_ids, encoding);
        let stream = self
            .authorize(self.client.get(self.endpoint(PRICE_STREAM)?).query(&params))
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
            .authorize(self.client.get(self.endpoint(PRICE_LATEST)?).query(&params))
            .send()
            .await?
            .json()
            .await?;
        Ok(update)
    }

    /// Get price updates at a specific publish time.
    ///
    /// Returns the first update whose `publish_time` is >= the provided value.
    #[cfg(feature = "nightly-pyth-historical-api")]
    pub async fn historical_price_updates(
        &self,
        feed_ids: impl IntoIterator<Item = &Identifier>,
        publish_time: i64,
        encoding: Option<EncodingType>,
    ) -> crate::Result<PriceUpdate> {
        let params = get_query(feed_ids, encoding);
        let path = format!("{PRICE_HISTORICAL}{publish_time}");
        let update = self
            .authorize(self.client.get(self.endpoint(&path)?).query(&params))
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
                    .ok_or(crate::Error::custom(format!("missing price for {feed}")))?;
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
        Self::try_new(DEFAULT_HERMES_BASE).expect("default Hermes base must be valid")
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
    /// Get the parsed price update.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_feed_id() -> Identifier {
        Identifier::from_hex("ff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace")
            .unwrap()
    }

    #[test]
    fn debug_redacts_api_key() {
        let hermes = Hermes::try_new_with_api_key(DEFAULT_HERMES_BASE, "super-secret-key").unwrap();
        let debug = format!("{hermes:?}");
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("[redacted]"));
    }

    /// Build the request exactly as the client methods do, so the assertions
    /// cover the real URL and headers.
    fn build_request(hermes: &Hermes, path: &str) -> reqwest::Request {
        let params = get_query([&sample_feed_id()], None);
        hermes
            .authorize(
                hermes
                    .client
                    .get(hermes.endpoint(path).unwrap())
                    .query(&params),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn latest_request_includes_bearer_token() {
        let hermes =
            Hermes::try_new_with_api_key("https://example.com/hermes", "test-token").unwrap();
        let request = build_request(&hermes, PRICE_LATEST);

        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer test-token")
        );
        assert_eq!(request.url().path(), "/hermes/v2/updates/price/latest");
        assert!(request.url().query().unwrap().contains("ids"));
    }

    #[test]
    fn stream_request_includes_bearer_token() {
        let hermes =
            Hermes::try_new_with_api_key("https://example.com/hermes", "test-token").unwrap();
        let request = build_request(&hermes, PRICE_STREAM);

        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer test-token")
        );
        assert_eq!(request.url().path(), "/hermes/v2/updates/price/stream");
        assert!(request.url().query().unwrap().contains("ids"));
    }

    #[test]
    fn request_omits_authorization_without_api_key() {
        let hermes = Hermes::try_new("https://example.com/hermes").unwrap();
        let request = build_request(&hermes, PRICE_LATEST);

        assert!(request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_none());
    }

    #[test]
    fn base_path_prefix_is_kept_with_or_without_trailing_slash() {
        for base in ["https://example.com/hermes", "https://example.com/hermes/"] {
            let hermes = Hermes::try_new(base).unwrap();
            assert_eq!(
                hermes.endpoint(PRICE_LATEST).unwrap().as_str(),
                "https://example.com/hermes/v2/updates/price/latest",
                "base {base}"
            );
            assert_eq!(
                hermes.endpoint(PRICE_STREAM).unwrap().as_str(),
                "https://example.com/hermes/v2/updates/price/stream",
                "base {base}"
            );
        }
    }

    #[test]
    fn default_base_resolves_to_hermes_pyth_network() {
        let hermes = Hermes::default();
        assert_eq!(
            hermes.endpoint(PRICE_LATEST).unwrap().as_str(),
            "https://hermes.pyth.network/v2/updates/price/latest"
        );
        assert_eq!(
            hermes.endpoint(PRICE_STREAM).unwrap().as_str(),
            "https://hermes.pyth.network/v2/updates/price/stream"
        );
    }

    #[test]
    fn nested_base_path_is_kept() {
        let hermes = Hermes::try_new("https://example.com/a/b").unwrap();
        assert_eq!(
            hermes.endpoint(PRICE_LATEST).unwrap().as_str(),
            "https://example.com/a/b/v2/updates/price/latest"
        );
    }

    #[cfg(feature = "nightly-pyth-historical-api")]
    #[test]
    fn historical_path_keeps_base_prefix() {
        let hermes = Hermes::try_new("https://example.com/hermes").unwrap();
        let path = format!("{PRICE_HISTORICAL}1700000000");
        assert_eq!(
            hermes.endpoint(&path).unwrap().as_str(),
            "https://example.com/hermes/v2/updates/price/1700000000"
        );
    }

    #[test]
    fn from_default_envs_requires_pyth_api_key() {
        match std::env::var(ENV_API_KEY) {
            Ok(key) => {
                let hermes = Hermes::from_default_envs().unwrap();
                assert_eq!(hermes.api_key.as_deref(), Some(key.as_str()));
                assert!(hermes.base.as_str().starts_with(DEFAULT_HERMES_BASE));
            }
            Err(_) => {
                assert!(Hermes::from_default_envs().is_err());
            }
        }
    }

    /// Live request. Hermes rejects unauthenticated price requests since the
    /// Pyth Core upgrade, so this only runs when `PYTH_API_KEY` is set.
    #[cfg(feature = "nightly-pyth-historical-api")]
    #[tokio::test]
    async fn test_historical_price_updates() -> eyre::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let Ok(hermes) = Hermes::from_default_envs() else {
            eprintln!("skipping test_historical_price_updates: {ENV_API_KEY} is not set");
            return Ok(());
        };

        // ETH/USD feed
        let feed_id = sample_feed_id();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let publish_time = now - 300;
        let update = hermes
            .historical_price_updates(&[feed_id], publish_time, None)
            .await?;
        assert!(!update.parsed().is_empty());
        let first = &update.parsed()[0];
        assert!(first.price().publish_time() >= publish_time);
        Ok(())
    }
}
