//! HTTP RPC sender implementation.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::{Duration, Instant},
};

use backon::{DefaultSleeper, Sleeper};
use reqwest::{header, StatusCode};
use solana_rpc_client_api::{
    client_error, custom_error,
    error_object::RpcErrorObject,
    request::{RpcError, RpcResponseErrorData},
    response::RpcSimulateTransactionResult,
};

use super::{RpcSender, RpcTransportStats};

/// HTTP RPC sender implementation.
pub struct HttpRpcSender {
    client: Arc<reqwest::Client>,
    url: String,
    request_id: AtomicU64,
    stats: RwLock<RpcTransportStats>,
    // Use backon::DefaultSleeper for cross-platform sleep.
    sleeper: DefaultSleeper,
}

impl HttpRpcSender {
    /// Create an HTTP RPC sender with default reqwest client.
    pub fn new(url: impl ToString) -> Self {
        Self::new_with_client(url, Default::default())
    }

    /// Create an HTTP RPC sender.
    pub fn new_with_client(url: impl ToString, client: reqwest::Client) -> Self {
        Self {
            client: Arc::new(client),
            url: url.to_string(),
            request_id: Default::default(),
            stats: Default::default(),
            sleeper: DefaultSleeper::default(),
        }
    }

    /// Create default headers used by HTTP sender.
    pub fn default_headers() -> header::HeaderMap {
        header::HeaderMap::new()
    }
}

struct StatsUpdater<'a> {
    stats: &'a RwLock<RpcTransportStats>,
    request_start_time: Instant,
    rate_limited_time: Duration,
}

impl<'a> StatsUpdater<'a> {
    fn new(stats: &'a RwLock<RpcTransportStats>) -> Self {
        Self {
            stats,
            request_start_time: Instant::now(),
            rate_limited_time: Duration::default(),
        }
    }

    fn add_rate_limited_time(&mut self, duration: Duration) {
        self.rate_limited_time += duration;
    }
}

impl Drop for StatsUpdater<'_> {
    fn drop(&mut self) {
        let mut stats = self.stats.write().unwrap();
        stats.request_count += 1;
        stats.elapsed_time += Instant::now().duration_since(self.request_start_time);
        stats.rate_limited_time += self.rate_limited_time;
    }
}

impl RpcSender for HttpRpcSender {
    async fn send(
        &self,
        request: super::RpcRequest,
        params: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        let mut stats_updater = StatsUpdater::new(&self.stats);

        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);
        let request_json = request.build_request_json(request_id, params).to_string();

        let mut too_many_requests_retries = 5;
        loop {
            let response = {
                let client = self.client.clone();
                let request_json = request_json.clone();
                client
                    .post(&self.url)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(request_json)
                    .send()
                    .await
            }?;

            if !response.status().is_success() {
                if response.status() == StatusCode::TOO_MANY_REQUESTS
                    && too_many_requests_retries > 0
                {
                    let mut duration = Duration::from_millis(500);
                    if let Some(retry_after) = response.headers().get(header::RETRY_AFTER) {
                        if let Ok(retry_after) = retry_after.to_str() {
                            if let Ok(retry_after) = retry_after.parse::<u64>() {
                                if retry_after < 120 {
                                    duration = Duration::from_secs(retry_after);
                                }
                            }
                        }
                    }

                    too_many_requests_retries -= 1;
                    tracing::debug!(
                        "Too many requests: server responded with {:?}, {} retries left, pausing for {:?}",
                        response, too_many_requests_retries, duration
                    );

                    self.sleeper.sleep(duration).await;
                    stats_updater.add_rate_limited_time(duration);
                    continue;
                }
                return Err(response.error_for_status().unwrap_err().into());
            }

            let mut json = response.json::<serde_json::Value>().await?;
            if json["error"].is_object() {
                return match serde_json::from_value::<RpcErrorObject>(json["error"].clone()) {
                    Ok(rpc_error_object) => {
                        let data = match rpc_error_object.code {
                                    custom_error::JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE => {
                                        match serde_json::from_value::<RpcSimulateTransactionResult>(json["error"]["data"].clone()) {
                                            Ok(data) => RpcResponseErrorData::SendTransactionPreflightFailure(data),
                                            Err(err) => {
                                                tracing::debug!("Failed to deserialize RpcSimulateTransactionResult: {:?}", err);
                                                RpcResponseErrorData::Empty
                                            }
                                        }
                                    },
                                    custom_error::JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY => {
                                        match serde_json::from_value::<custom_error::NodeUnhealthyErrorData>(json["error"]["data"].clone()) {
                                            Ok(custom_error::NodeUnhealthyErrorData {num_slots_behind}) => RpcResponseErrorData::NodeUnhealthy {num_slots_behind},
                                            Err(_err) => {
                                                RpcResponseErrorData::Empty
                                            }
                                        }
                                    },
                                    _ => RpcResponseErrorData::Empty
                                };

                        Err(client_error::Error::from(RpcError::RpcResponseError {
                            code: rpc_error_object.code,
                            message: rpc_error_object.message,
                            data,
                        })
                        .into())
                    }
                    Err(err) => Err(client_error::Error::from(RpcError::RpcRequestError(format!(
                        "Failed to deserialize RPC error response: {} [{}]",
                        serde_json::to_string(&json["error"]).unwrap(),
                        err
                    )))
                    .into()),
                };
            }
            return Ok(json["result"].take());
        }
    }

    fn get_transport_stats(&self) -> RpcTransportStats {
        self.stats.read().unwrap().clone()
    }

    fn url(&self) -> String {
        self.url.clone()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use crate::{
        client_traits::{HttpRpcSender, RpcSender},
        cluster::Cluster,
        solana_rpc_client_api::request::RpcRequest,
    };

    #[test]
    async fn send_request() -> crate::Result<()> {
        let cluster = Cluster::Devnet;

        let sender = HttpRpcSender::new(cluster.url());

        let response = sender
            .send(RpcRequest::GetVersion, serde_json::Value::Null)
            .await?;

        println!("{response:?}");

        Ok(())
    }
}
