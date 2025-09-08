/// Error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Parse url error.
    #[error("parse url: {0}")]
    ParseUrl(#[from] url::ParseError),
    /// Parse cluster error.
    #[error("parse cluster: {0}")]
    ParseCluster(&'static str),
    /// Merge transaction error.
    #[error("merge transaction: {0}")]
    MergeTransaction(&'static str),
    /// Add transaction error.
    #[error("add transaction: {0}")]
    AddTransaction(&'static str),
    /// Compile message error.
    #[error("compile message: {0}")]
    CompileMessage(#[from] solana_sdk::message::CompileError),
    /// Client error.
    #[cfg(feature = "solana-client")]
    #[error("client: {0}")]
    Client(#[from] Box<solana_client::client_error::ClientError>),
    /// Signer error.
    #[error("signer: {0}")]
    Signer(#[from] solana_sdk::signer::SignerError),
    /// Custom error.
    #[error("custom: {0}")]
    Custom(String),
    /// RPC client error.
    #[cfg(feature = "solana-rpc-client-api")]
    #[error("rpc-client-api: {0}")]
    RpcClientApi(Box<solana_rpc_client_api::client_error::Error>),
    /// JSON error.
    #[cfg(feature = "serde_json")]
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Anchor error.
    #[cfg(feature = "anchor-lang")]
    #[error("anchor: {0}")]
    Anchor(#[from] anchor_lang::error::Error),
    /// Reqwest error.
    #[cfg(feature = "reqwest")]
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
}

impl<T> From<(T, Error)> for Error {
    fn from(value: (T, crate::Error)) -> Self {
        value.1
    }
}

impl Error {
    /// Create a custom error.
    pub fn custom(msg: impl ToString) -> Self {
        Self::Custom(msg.to_string())
    }
}

#[cfg(feature = "solana-rpc-client-api")]
impl From<solana_rpc_client_api::client_error::Error> for Error {
    fn from(err: solana_rpc_client_api::client_error::Error) -> Self {
        Self::RpcClientApi(Box::new(err))
    }
}
