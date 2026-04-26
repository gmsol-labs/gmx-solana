//! Default timeouts for outbound HTTP clients.
//!
//! [`reqwest::Client::new`] applies no connect or request timeout, so a stuck peer can block
//! async workers indefinitely. These helpers set conservative bounds for RPC-style calls while
//! leaving long-lived streams (Hermes SSE, Chainlink WS) without a full-response timeout.

use std::time::Duration;

use reqwest::Client;

/// Connect handshake bound for all outbound HTTP integrations in this crate.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Idle connection eviction for the reqwest connection pool.
pub(crate) const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Typical REST / JSON response (Hermes latest price, Chaos recommendations, etc.).
pub(crate) const REST_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const TCP_KEEPALIVE: Duration = Duration::from_secs(60);

/// HTTP client for request/response calls where the full body should finish within
/// [`REST_REQUEST_TIMEOUT`].
pub(crate) fn bounded_rest_client() -> Client {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REST_REQUEST_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .tcp_keepalive(TCP_KEEPALIVE)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// HTTP client for long-lived streams (SSE, WebSocket upgrade). Only the connect phase is
/// strictly bounded; reading the body can take as long as the stream stays open.
pub(crate) fn streaming_http_client() -> Client {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .tcp_keepalive(TCP_KEEPALIVE)
        .build()
        .unwrap_or_else(|_| Client::new())
}
