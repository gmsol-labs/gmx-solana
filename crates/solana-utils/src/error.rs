use solana_sdk::pubkey::Pubkey;

/// Error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Account Not Found.
    #[error("account not found: {0}")]
    AccountNotFound(Pubkey),
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
    /// Remaining transactions were not sent after an earlier failure.
    #[error("send aborted after transaction {failed_at} failed")]
    SendAborted {
        /// Index of the transaction whose failure stopped the bundle send.
        failed_at: usize,
    },
    /// The transaction was submitted, but confirming it failed (blockhash expired, or an RPC
    /// error while polling its status). It may or may not have landed: look up `signature`
    /// before retrying, or the action may execute twice.
    #[cfg(feature = "solana-client")]
    #[error("transaction {signature} was submitted but its outcome is unknown: {source}")]
    SendUnresolved {
        /// Signature of the submitted transaction.
        signature: solana_sdk::signature::Signature,
        /// Why confirmation failed.
        source: Box<solana_client::client_error::ClientError>,
    },
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

    /// Signature of a submitted transaction whose outcome is unknown
    /// ([`Error::SendUnresolved`]), or `None` for any other error.
    #[cfg(feature = "solana-client")]
    pub fn unresolved_signature(&self) -> Option<&solana_sdk::signature::Signature> {
        match self {
            Self::SendUnresolved { signature, .. } => Some(signature),
            _ => None,
        }
    }
}

#[cfg(feature = "solana-rpc-client-api")]
impl From<solana_rpc_client_api::client_error::Error> for Error {
    fn from(err: solana_rpc_client_api::client_error::Error) -> Self {
        Self::RpcClientApi(Box::new(err))
    }
}
