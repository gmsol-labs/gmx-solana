use std::{fmt, ops::Deref, sync::Arc};

use chainlink_datastreams::report::{decode, decode_full_report, Report};
use futures_util::{Stream, StreamExt, TryStreamExt};
use reqwest::{IntoUrl, Url};
use reqwest_websocket::{Message, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// ENV for User ID.
pub const ENV_USER_ID: &str = "CHAINLINK_USER_ID";

/// ENV for Secret.
pub const ENV_SECRET: &str = "CHAINLINK_SECRET";

/// Default base URL for Chainlink Streams.
pub const DEFAULT_STREAMS_BASE: &str = "https://api.dataengine.chain.link";

/// Testnet base URL for Chainlink Streams.
pub const TESTNET_STREAMS_BASE: &str = "https://api.testnet-dataengine.chain.link";

/// Default base URL for Chainlink Streams.
pub const DEFAULT_WS_STREAMS_BASE: &str = "wss://ws.dataengine.chain.link";

/// Testnet base URL for Chainlink Streams.
pub const TESTNET_WS_STREAMS_BASE: &str = "wss://ws.testnet-dataengine.chain.link";

enum Path {
    ReportsLatest,
    ReportsBulk,
    Feeds,
    Websocket,
}

impl Path {
    fn to_uri(&self) -> &str {
        match self {
            Self::ReportsLatest => "/api/v1/reports/latest",
            Self::ReportsBulk => "/api/v1/reports/bulk",
            Self::Feeds => "/api/v1/feeds",
            Self::Websocket => "/api/v1/ws",
        }
    }
}

/// Credential.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    user_id: String,
    secret: String,
}

impl Credential {
    /// Create from Default ENVs.
    pub fn from_default_envs() -> crate::Result<Self> {
        use std::env;

        let user_id = env::var(ENV_USER_ID).map_err(crate::Error::invalid_argument)?;
        let secret = env::var(ENV_SECRET).map_err(crate::Error::invalid_argument)?;

        Ok(Self { user_id, secret })
    }

    fn generate_hmac(&self, timestamp: i128, request: &reqwest::Request) -> crate::Result<String> {
        use hmac::{Hmac, Mac};

        let body = request
            .body()
            .and_then(|body| body.as_bytes())
            .unwrap_or_default();
        let body_hash = hex::encode(Sha256::digest(body));

        let url = request.url();
        let uri = std::iter::once(url.path())
            .chain(url.query())
            .collect::<Vec<_>>()
            .join("?");

        let message = format!(
            "{} {uri} {body_hash} {} {timestamp}",
            request.method(),
            self.user_id
        );

        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
            .map_err(crate::Error::invalid_argument)?;
        mac.update(message.as_bytes());

        let signature = hex::encode(mac.finalize().into_bytes());

        Ok(signature)
    }

    fn sign(&self, request: &mut reqwest::Request) -> crate::Result<()> {
        let timestamp_nanos = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
        let timestamp = timestamp_nanos / 1_000_000;

        let signature = self.generate_hmac(timestamp, request)?;
        let header = request.headers_mut();
        header.insert(
            "Authorization",
            self.user_id
                .parse()
                .map_err(crate::Error::invalid_argument)?,
        );
        header.insert(
            "X-Authorization-Timestamp",
            timestamp
                .to_string()
                .parse()
                .map_err(crate::Error::invalid_argument)?,
        );
        header.insert(
            "X-Authorization-Signature-SHA256",
            signature.parse().map_err(crate::Error::invalid_argument)?,
        );
        Ok(())
    }
}

/// Chainlink Data Streams Client.
#[derive(Clone)]
pub struct Client {
    base: Url,
    ws_base: Url,
    client: reqwest::Client,
    credential: Arc<Credential>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .field("ws_base", &self.ws_base)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Create a new [`Client`] with the given base URL.
    pub fn try_new(
        base: impl IntoUrl,
        ws_base: impl IntoUrl,
        credential: Credential,
    ) -> crate::Result<Self> {
        Ok(Self {
            base: base.into_url()?,
            ws_base: ws_base.into_url()?,
            client: reqwest::Client::new(),
            credential: Arc::new(credential),
        })
    }

    /// Create a new mainnet [`Client`] with the given credential.
    pub fn with_credential(credential: Credential) -> Self {
        Self::try_new(DEFAULT_STREAMS_BASE, DEFAULT_WS_STREAMS_BASE, credential).unwrap()
    }

    /// Create a new testnet [`Client`] with the given credential.
    pub fn with_testnet_credential(credential: Credential) -> Self {
        Self::try_new(TESTNET_STREAMS_BASE, TESTNET_WS_STREAMS_BASE, credential).unwrap()
    }

    /// Create a new [`Client`] with default base url and default ENVs.
    pub fn from_defaults() -> crate::Result<Self> {
        Ok(Self::with_credential(Credential::from_default_envs()?))
    }

    /// Create a new [`Client`] with testnest base url and default ENVs.
    pub fn from_testnet_defaults() -> crate::Result<Self> {
        Ok(Self::with_testnet_credential(
            Credential::from_default_envs()?,
        ))
    }

    fn get_inner<T>(
        &self,
        path: Path,
        query: &T,
        sign: bool,
        ws: bool,
    ) -> crate::Result<reqwest::RequestBuilder>
    where
        T: Serialize,
    {
        let base = if ws { &self.ws_base } else { &self.base };
        let url = base.join(path.to_uri())?;
        let mut request = self.client.get(url).query(query).build()?;
        if sign {
            self.credential.sign(&mut request)?;
        }
        Ok(reqwest::RequestBuilder::from_parts(
            self.client.clone(),
            request,
        ))
    }

    fn get<T>(&self, path: Path, query: &T, sign: bool) -> crate::Result<reqwest::RequestBuilder>
    where
        T: Serialize,
    {
        self.get_inner(path, query, sign, false)
    }

    /// Get available feeds.
    pub async fn feeds(&self) -> crate::Result<Feeds> {
        let feeds = self
            .get::<Option<()>>(Path::Feeds, &None, true)?
            .send()
            .await?
            .json()
            .await?;
        Ok(feeds)
    }

    /// Get latest report of the given hex-encoded feed ID.
    pub async fn latest_report(&self, feed_id: &str) -> crate::Result<ApiReport> {
        let report = self
            .get(Path::ReportsLatest, &[("feedID", feed_id)], true)?
            .send()
            .await?
            .json()
            .await?;
        Ok(report)
    }

    /// Get bulk of reports with the given feed IDs and timestamp.
    pub async fn bulk_report(
        &self,
        feed_ids: impl IntoIterator<Item = &str>,
        ts: time::OffsetDateTime,
    ) -> crate::Result<ApiReports> {
        let feed_ids = feed_ids.into_iter().collect::<Vec<_>>().join(",");
        let timestamp = ts.unix_timestamp();
        let reports = self
            .get(
                Path::ReportsBulk,
                &[("feedIDs", feed_ids), ("timestamp", timestamp.to_string())],
                true,
            )?
            .send()
            .await?
            .json()
            .await?;
        Ok(reports)
    }

    /// Subscribe to report updates using websocket.
    pub async fn subscribe(
        &self,
        feed_ids: impl IntoIterator<Item = &str>,
    ) -> crate::Result<impl Stream<Item = crate::Result<ApiReport>>> {
        let feed_ids = feed_ids.into_iter().collect::<Vec<_>>().join(",");
        let ws = self
            .get_inner(Path::Websocket, &[("feedIDs", feed_ids)], true, true)?
            .upgrade()
            .send()
            .await
            .map_err(crate::Error::transport)?
            .into_websocket()
            .await
            .map_err(crate::Error::transport)?;

        let stream = ws
            .map_err(crate::Error::transport)
            .and_then(|message| async {
                match message {
                    Message::Binary(data) => Ok(Some(data)),
                    Message::Close { code, reason } => Err(crate::Error::transport(format!(
                        "channel closed: code = {code}, reason = {reason}"
                    ))),
                    _ => Ok(None),
                }
            })
            .filter_map(|message| async { message.transpose() })
            .and_then(|data| async move {
                let report = serde_json::from_slice(&data)?;
                Ok(report)
            });

        Ok(stream)
    }
}

/// Feeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feeds {
    /// Feeds.
    pub feeds: Vec<Feed>,
}

/// Feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    /// Hex-encoded Feed ID.
    #[serde(rename = "feedID")]
    pub feed_id: String,
}

/// Raw Report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiReport {
    report: ApiReportData,
}

impl ApiReport {
    /// Into report data.
    pub fn into_data(self) -> ApiReportData {
        self.report
    }
}

impl Deref for ApiReport {
    type Target = ApiReportData;

    fn deref(&self) -> &Self::Target {
        &self.report
    }
}

/// A bulk of reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiReports {
    reports: Vec<ApiReportData>,
}

impl ApiReports {
    /// Into reports.
    pub fn into_reports(self) -> Vec<ApiReportData> {
        self.reports
    }
}

impl Deref for ApiReports {
    type Target = Vec<ApiReportData>;

    fn deref(&self) -> &Self::Target {
        &self.reports
    }
}

/// Raw Report Data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiReportData {
    /// Feed ID.
    #[serde(rename = "feedID")]
    pub feed_id: String,
    /// Full Report.
    pub full_report: String,
    /// Observations timestamp (in secs).
    pub observations_timestamp: i64,
    /// Valid From Timestamp (in secs).
    pub valid_from_timestamp: i64,
}

impl ApiReportData {
    /// Decode the report.
    pub fn decode(&self) -> crate::Result<Report> {
        let report = self.report_bytes()?;
        let (_, blob) = decode_full_report(&report).map_err(crate::Error::invalid_argument)?;
        let report = decode(blob).map_err(crate::Error::invalid_argument)?;
        Ok(report)
    }

    /// Decode report to bytes.
    pub fn report_bytes(&self) -> crate::Result<Vec<u8>> {
        hex::decode(
            self.full_report
                .strip_prefix("0x")
                .unwrap_or(&self.full_report),
        )
        .map_err(crate::Error::invalid_argument)
    }

    /// Feed ID.
    pub fn decode_feed_id(&self) -> crate::Result<[u8; 32]> {
        let mut data = [0; 32];
        hex::decode_to_slice(
            self.feed_id.strip_prefix("0x").unwrap_or(&self.feed_id),
            &mut data,
        )
        .map_err(crate::Error::unknown)?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_hmac() {
        let credential = Credential {
            user_id: "clientId2".to_string(),
            secret: "secret2".to_string(),
        };

        let client = reqwest::Client::new();
        let request = client
            .post(format!(
                "{DEFAULT_STREAMS_BASE}{}",
                Path::ReportsBulk.to_uri()
            ))
            .body(r#"{"attr1": "value1","attr2": [1,2,3]}"#)
            .build()
            .unwrap();

        let signature = credential.generate_hmac(1718885772, &request).unwrap();
        assert_eq!(
            signature,
            "37190febe20b6f3662f6abbfa3a7085ad705ac64e88bde8c1a01a635859e6cf7"
        );
    }
}
