use std::ops::Deref;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signer::Signer};

use crate::cluster::Cluster;

/// Wallet Config.
#[derive(Clone)]
pub struct Config<C> {
    cluster: Cluster,
    payer: C,
    options: CommitmentConfig,
}

impl<C> Config<C> {
    /// Create a new wallet config.
    pub fn new(cluster: Cluster, payer: C, options: CommitmentConfig) -> Self {
        Self {
            cluster,
            payer,
            options,
        }
    }

    /// Get cluster.
    pub fn cluster(&self) -> &Cluster {
        &self.cluster
    }

    /// Get commitment config.
    pub fn commitment(&self) -> &CommitmentConfig {
        &self.options
    }

    /// Create a Solana RPC Client.
    pub fn rpc(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.cluster.url().to_string(), self.options)
    }

    /// Set payer.
    pub fn set_payer<C2>(self, payer: C2) -> Config<C2> {
        Config {
            cluster: self.cluster,
            payer,
            options: self.options,
        }
    }

    /// Set cluster.
    pub fn set_cluster(mut self, url: impl AsRef<str>) -> crate::Result<Self> {
        self.cluster = url.as_ref().parse()?;
        Ok(self)
    }

    /// Set options.
    pub fn set_options(mut self, options: CommitmentConfig) -> Self {
        self.options = options;
        self
    }
}

impl<C: Deref<Target = impl Signer>> Config<C> {
    /// Get payer pubkey.
    pub fn payer(&self) -> Pubkey {
        self.payer.pubkey()
    }
}
