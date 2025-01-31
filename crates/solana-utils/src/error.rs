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
    #[error("client: {0}")]
    Client(#[from] Box<solana_client::client_error::ClientError>),
    /// Signer error.
    #[error("signer: {0}")]
    Signer(#[from] solana_sdk::signer::SignerError),
}

impl<T> From<(T, Error)> for Error {
    fn from(value: (T, crate::Error)) -> Self {
        value.1
    }
}
