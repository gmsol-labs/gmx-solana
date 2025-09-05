//! A transport for RPC calls.
// Inspired by solana-rpc-client v2.1.21 (Apache-2.0).
// Reimplemented here to avoid dependency on `tokio`.

use solana_rpc_client_api::request::RpcRequest;
use std::{future::Future, time::Duration};

/// Type describing the status of RPC transport.
#[derive(Default, Clone)]
pub struct RpcTransportStats {
    /// Number of RPC requests issued.
    pub request_count: usize,

    /// Total amount of time spent transacting with the RPC server.
    pub elapsed_time: Duration,

    /// Total amount of waiting time due to RPC server rate limiting
    /// (a subset of `elapsed_time`)
    pub rate_limited_time: Duration,
}

/// A transport for RPC calls.
/// `RpcSender` implements the underlying transport of requests to, and
/// responses from, a Solana node.
pub trait RpcSender {
    /// Send an [`RpcRequest`] with JSON parameters.
    fn send(
        &self,
        request: RpcRequest,
        params: serde_json::Value,
    ) -> impl Future<Output = crate::Result<serde_json::Value>>;

    /// Get RPC transport statistics.
    fn get_transport_stats(&self) -> RpcTransportStats;

    /// Get the RPC endpoint URL.
    fn url(&self) -> String;
}
