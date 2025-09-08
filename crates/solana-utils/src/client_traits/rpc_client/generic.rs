//! Generic RPC client implementation.

use std::time::Duration;

use serde::{de::DeserializeOwned, Serialize};
use solana_rpc_client_api::request::RpcRequest;
use solana_sdk::commitment_config::CommitmentConfig;

use crate::client_traits::RpcSender;

use super::RpcClient;

#[cfg(http_rpc_sender)]
use crate::client_traits::rpc_sender::HttpRpcSender;

/// Generic RPC client configuration.
#[derive(Debug, Default, Clone)]
pub struct GenericRpcClientConfig {
    /// Commitment level for RPC queries. See [`CommitmentConfig`].
    pub commitment_config: CommitmentConfig,
    /// Initial timeout for transaction confirmation; `None` uses the client default.
    pub confirm_transaction_initial_timeout: Option<Duration>,
}

/// Generic RPC client implementation.
#[derive(Debug, Clone)]
pub struct GenericRpcClient<S> {
    sender: S,
    config: GenericRpcClientConfig,
}

impl<S> GenericRpcClient<S> {
    /// Create a RPC client with sender and config.
    pub fn new_with_sender_and_config(sender: S, config: GenericRpcClientConfig) -> Self {
        Self { sender, config }
    }
}

#[cfg(http_rpc_sender)]
impl GenericRpcClient<HttpRpcSender> {
    /// Create a RPC client with the given url and commitment config.
    pub fn new_with_commitment(url: impl ToString, commitment: CommitmentConfig) -> Self {
        Self::new_with_sender_and_config(
            HttpRpcSender::new(url),
            GenericRpcClientConfig {
                commitment_config: commitment,
                ..Default::default()
            },
        )
    }

    /// Create a RPC client.
    pub fn new(url: impl ToString) -> Self {
        Self::new_with_commitment(url, Default::default())
    }
}

impl<S: RpcSender> RpcClient for GenericRpcClient<S> {
    /// Returns the configured default commitment level.
    fn commitment(&self) -> CommitmentConfig {
        self.config.commitment_config
    }

    /// Send an [`RpcRequest`] with parameters.
    async fn send<T>(&self, request: RpcRequest, params: impl Serialize) -> crate::Result<T>
    where
        T: DeserializeOwned,
    {
        let params = serde_json::to_value(params)?;
        if !params.is_array() && !params.is_null() {
            return Err(crate::Error::custom(
                "`params` is neither an array nor null",
            ));
        }

        let response = self.sender.send(request, params).await?;
        Ok(serde_json::from_value(response)?)
    }
}
