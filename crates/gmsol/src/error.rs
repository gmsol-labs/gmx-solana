use anchor_client::{solana_client::pubsub_client::PubsubClientError, solana_sdk};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

pub use gmsol_store::CoreError;

/// Error type for `gmsol`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Empty deposit.
    #[error("empty deposit")]
    EmptyDeposit,
    /// Anchor Error.
    #[error("anchor: {0:#?}")]
    Anchor(AnchorError),
    /// Client Error.
    #[error("{0:#?}")]
    Client(anchor_client::ClientError),
    /// Model error.
    #[error("model: {0}")]
    Model(#[from] gmsol_model::Error),
    /// Number out of range.
    #[error("numer out of range")]
    NumberOutOfRange,
    /// Unknown errors.
    #[error("unknown: {0}")]
    Unknown(String),
    /// Eyre errors.
    #[error("eyre: {0}")]
    Eyre(#[from] eyre::Error),
    /// Missing return data.
    #[error("missing return data")]
    MissingReturnData,
    /// Base64 Decode Error.
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// IO Error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Bytemuck error.
    #[error("bytemuck: {0}")]
    Bytemuck(bytemuck::PodCastError),
    /// Invalid Arguments.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Format error.
    #[error("fmt: {0}")]
    Fmt(#[from] std::fmt::Error),
    /// Reqwest error.
    #[cfg(feature = "reqwest")]
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    /// Parse url error.
    #[error("parse url: {0}")]
    ParseUrl(#[from] url::ParseError),
    /// SSE error.
    #[cfg(all(feature = "eventsource-stream", feature = "reqwest"))]
    #[error("sse: {0}")]
    Sse(#[from] eventsource_stream::EventStreamError<reqwest::Error>),
    /// JSON error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Decode error.
    #[cfg(feature = "decode")]
    #[error("decode: {0}")]
    Decode(#[from] gmsol_decode::DecodeError),
    /// Lagged.
    #[error("lagged: {0}")]
    Lagged(#[from] BroadcastStreamRecvError),
    /// Pubsub client closed.
    #[error("pubsub: closed")]
    PubsubClosed,
    /// Complie Solana Message Error.
    #[error("compile message: {0}")]
    CompileMessage(#[from] solana_sdk::message::CompileError),
    /// Signer Error.
    #[error("signer: {0}")]
    SignerError(#[from] solana_sdk::signer::SignerError),
    /// Transport Error.
    #[error("transport: {0}")]
    Transport(String),
    /// Switchboard Error.
    #[error("switchboard: {0}")]
    Switchboard(String),
    /// Solana utils error.
    #[error(transparent)]
    SolanaUtils(gmsol_solana_utils::Error),
}

impl Error {
    /// Create unknown error.
    pub fn unknown(msg: impl ToString) -> Self {
        Self::Unknown(msg.to_string())
    }

    /// Create an "invalid argument" error.
    pub fn invalid_argument(msg: impl ToString) -> Self {
        Self::InvalidArgument(msg.to_string())
    }

    /// Create a transport error.
    pub fn transport(msg: impl ToString) -> Self {
        Self::Transport(msg.to_string())
    }

    /// Create a switchboard error.
    pub fn switchboard_error(msg: impl ToString) -> Self {
        Self::Switchboard(msg.to_string())
    }

    /// Anchor Error Code.
    pub fn anchor_error_code(&self) -> Option<u32> {
        let Self::Anchor(error) = self else {
            return None;
        };
        Some(error.error_code_number)
    }
}

impl From<anchor_client::ClientError> for Error {
    fn from(error: anchor_client::ClientError) -> Self {
        use anchor_client::ClientError;

        match error {
            ClientError::AccountNotFound => Self::NotFound,
            ClientError::SolanaClientError(error) => match handle_solana_client_error(&error) {
                Some(err) => err,
                None => Self::Client(ClientError::SolanaClientError(error)),
            },
            ClientError::SolanaClientPubsubError(err) => Self::from(err),
            err => Self::Client(err),
        }
    }
}

impl From<gmsol_solana_utils::Error> for Error {
    fn from(value: gmsol_solana_utils::Error) -> Self {
        match value {
            gmsol_solana_utils::Error::Client(err) => match handle_solana_client_error(&err) {
                Some(err) => err,
                None => Self::SolanaUtils(err.into()),
            },
            err => Self::SolanaUtils(err),
        }
    }
}

impl From<anchor_client::anchor_lang::error::Error> for Error {
    fn from(value: anchor_client::anchor_lang::error::Error) -> Self {
        Self::Client(value.into())
    }
}

impl From<PubsubClientError> for Error {
    fn from(err: PubsubClientError) -> Self {
        match err {
            PubsubClientError::ConnectionClosed(_) => Self::PubsubClosed,
            err => anchor_client::ClientError::from(err).into(),
        }
    }
}

/// Anchor Error with owned source.
#[derive(Debug)]
pub struct AnchorError {
    /// Error name.
    pub error_name: String,
    /// Error code.
    pub error_code_number: u32,
    /// Error message.
    pub error_msg: String,
    /// Error origin.
    pub error_origin: Option<ErrorOrigin>,
    /// Logs.
    pub logs: Vec<String>,
}

/// Error origin with owned source.
#[derive(Debug)]
pub enum ErrorOrigin {
    /// Source.
    Source(String, u32),
    /// Account.
    AccountName(String),
}

fn handle_solana_client_error(
    error: &anchor_client::solana_client::client_error::ClientError,
) -> Option<Error> {
    use anchor_client::solana_client::{
        client_error::ClientErrorKind,
        rpc_request::{RpcError, RpcResponseErrorData},
    };

    let ClientErrorKind::RpcError(rpc_error) = error.kind() else {
        return None;
    };

    let RpcError::RpcResponseError { data, .. } = rpc_error else {
        return None;
    };

    let RpcResponseErrorData::SendTransactionPreflightFailure(simulation) = data else {
        return None;
    };

    let Some(logs) = &simulation.logs else {
        return None;
    };

    for log in logs {
        if log.starts_with("Program log: AnchorError") {
            let log = log.trim_start_matches("Program log: AnchorError ");
            let Some((origin, rest)) = log.split_once("Error Code:") else {
                break;
            };
            let Some((name, rest)) = rest.split_once("Error Number:") else {
                break;
            };
            let Some((number, message)) = rest.split_once("Error Message:") else {
                break;
            };
            let number = number.trim().trim_end_matches('.');
            let Ok(number) = number.parse() else {
                break;
            };

            let origin = origin.trim().trim_end_matches('.');

            let origin = if origin.starts_with("thrown in") {
                let source = origin.trim_start_matches("thrown in ");
                if let Some((filename, line)) = source.split_once(':') {
                    Some(ErrorOrigin::Source(
                        filename.to_string(),
                        line.parse().ok().unwrap_or(0),
                    ))
                } else {
                    None
                }
            } else if origin.starts_with("caused by account:") {
                let account = origin.trim_start_matches("caused by account: ");
                Some(ErrorOrigin::AccountName(account.to_string()))
            } else {
                None
            };

            let error = AnchorError {
                error_name: name.trim().trim_end_matches('.').to_string(),
                error_code_number: number,
                error_msg: message.trim().to_string(),
                error_origin: origin,
                logs: logs.clone(),
            };

            return Some(Error::Anchor(error));
        }
    }

    None
}

impl<T> From<(T, gmsol_solana_utils::Error)> for Error {
    fn from((_, err): (T, gmsol_solana_utils::Error)) -> Self {
        Self::SolanaUtils(err)
    }
}
