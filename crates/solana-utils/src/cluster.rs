// Taken from:
// https://github.com/coral-xyz/anchor/blob/55d74c620d30fc3c088df71895a1956336825de4/client/src/cluster.rs

use std::str::FromStr;

use url::Url;

#[cfg(client)]
use solana_client::nonblocking::rpc_client::RpcClient;

#[cfg(client)]
use solana_sdk::commitment_config::CommitmentConfig;

/// Cluster.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
pub enum Cluster {
    /// Testnet.
    Testnet,
    /// Mainnet.
    Mainnet,
    /// Devnet.
    Devnet,
    /// Localnet.
    #[default]
    Localnet,
    /// Debug.
    Debug,
    /// Custom.
    Custom(String, String),
}

impl FromStr for Cluster {
    type Err = crate::Error;
    fn from_str(s: &str) -> crate::Result<Cluster> {
        match s.to_lowercase().as_str() {
            "t" | "testnet" => Ok(Cluster::Testnet),
            "m" | "mainnet" => Ok(Cluster::Mainnet),
            "d" | "devnet" => Ok(Cluster::Devnet),
            "l" | "localnet" => Ok(Cluster::Localnet),
            "g" | "debug" => Ok(Cluster::Debug),
            _ if s.starts_with("http") => {
                let http_url = s;

                // Taken from:
                // https://github.com/solana-labs/solana/blob/aea8f0df1610248d29d8ca3bc0d60e9fabc99e31/web3.js/src/util/url.ts

                let mut ws_url = Url::parse(http_url)?;
                if let Some(port) = ws_url.port() {
                    ws_url.set_port(Some(port + 1))
                        .map_err(|_| crate::Error::ParseCluster("Unable to set port"))?;
                }
                if ws_url.scheme() == "https" {
                    ws_url.set_scheme("wss")
                        .map_err(|_| crate::Error::ParseCluster("Unable to set scheme"))?;
                } else {
                    ws_url.set_scheme("ws")
                        .map_err(|_| crate::Error::ParseCluster("Unable to set scheme"))?;
                }


                Ok(Cluster::Custom(http_url.to_string(), ws_url.to_string()))
            }
            _ => Err(crate::Error::ParseCluster(
                "Cluster must be one of [localnet, testnet, mainnet, devnet] or be an http or https url\n",
            )),
        }
    }
}

impl std::fmt::Display for Cluster {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let clust_str = match self {
            Cluster::Testnet => "testnet",
            Cluster::Mainnet => "mainnet",
            Cluster::Devnet => "devnet",
            Cluster::Localnet => "localnet",
            Cluster::Debug => "debug",
            Cluster::Custom(url, _ws_url) => url,
        };
        write!(f, "{clust_str}")
    }
}

impl TryFrom<String> for Cluster {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Cluster> for String {
    fn from(value: Cluster) -> Self {
        value.to_string()
    }
}

impl Cluster {
    /// Get RPC url.
    pub fn url(&self) -> &str {
        match self {
            Cluster::Devnet => "https://api.devnet.solana.com",
            Cluster::Testnet => "https://api.testnet.solana.com",
            Cluster::Mainnet => "https://api.mainnet-beta.solana.com",
            Cluster::Localnet => "http://127.0.0.1:8899",
            Cluster::Debug => "http://34.90.18.145:8899",
            Cluster::Custom(url, _ws_url) => url,
        }
    }

    /// Get Websocket url.
    pub fn ws_url(&self) -> &str {
        match self {
            Cluster::Devnet => "wss://api.devnet.solana.com",
            Cluster::Testnet => "wss://api.testnet.solana.com",
            Cluster::Mainnet => "wss://api.mainnet-beta.solana.com",
            Cluster::Localnet => "ws://127.0.0.1:8900",
            Cluster::Debug => "ws://34.90.18.145:8900",
            Cluster::Custom(_url, ws_url) => ws_url,
        }
    }

    /// Create a Solana RPC Client.
    #[cfg(client)]
    pub fn rpc(&self, commitment: CommitmentConfig) -> RpcClient {
        RpcClient::new_with_commitment(self.url().to_string(), commitment)
    }
}

#[cfg(feature = "anchor")]
impl From<Cluster> for anchor_client::Cluster {
    fn from(cluster: Cluster) -> Self {
        match cluster {
            Cluster::Testnet => Self::Testnet,
            Cluster::Mainnet => Self::Mainnet,
            Cluster::Devnet => Self::Devnet,
            Cluster::Localnet => Self::Localnet,
            Cluster::Debug => Self::Debug,
            Cluster::Custom(url, ws_url) => Self::Custom(url, ws_url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cluster(name: &str, cluster: Cluster) {
        assert_eq!(Cluster::from_str(name).unwrap(), cluster);
    }

    #[test]
    fn test_cluster_parse() {
        test_cluster("testnet", Cluster::Testnet);
        test_cluster("mainnet", Cluster::Mainnet);
        test_cluster("devnet", Cluster::Devnet);
        test_cluster("localnet", Cluster::Localnet);
        test_cluster("debug", Cluster::Debug);
    }

    #[test]
    #[should_panic]
    fn test_cluster_bad_parse() {
        let bad_url = "httq://my_custom_url.test.net";
        Cluster::from_str(bad_url).unwrap();
    }

    #[test]
    fn test_http_port() {
        let url = "http://my-url.com:7000/";
        let cluster = Cluster::from_str(url).unwrap();
        assert_eq!(
            Cluster::Custom(url.to_string(), "ws://my-url.com:7001/".to_string()),
            cluster
        );
    }

    #[test]
    fn test_http_no_port() {
        let url = "http://my-url.com/";
        let cluster = Cluster::from_str(url).unwrap();
        assert_eq!(
            Cluster::Custom(url.to_string(), "ws://my-url.com/".to_string()),
            cluster
        );
    }

    #[test]
    fn test_https_port() {
        let url = "https://my-url.com:7000/";
        let cluster = Cluster::from_str(url).unwrap();
        assert_eq!(
            Cluster::Custom(url.to_string(), "wss://my-url.com:7001/".to_string()),
            cluster
        );
    }
    #[test]
    fn test_https_no_port() {
        let url = "https://my-url.com/";
        let cluster = Cluster::from_str(url).unwrap();
        assert_eq!(
            Cluster::Custom(url.to_string(), "wss://my-url.com/".to_string()),
            cluster
        );
    }

    #[test]
    fn test_upper_case() {
        let url = "http://my-url.com/FooBar";
        let cluster = Cluster::from_str(url).unwrap();
        assert_eq!(
            Cluster::Custom(url.to_string(), "ws://my-url.com/FooBar".to_string()),
            cluster
        );
    }
}
