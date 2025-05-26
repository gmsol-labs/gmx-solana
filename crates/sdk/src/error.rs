pub(crate) use gmsol_programs::anchor_lang::prelude::Error as AnchorLangError;

/// SDK Error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error from [`gmsol-solana-utils`].
    #[error("solana-utils: {0}")]
    SolanaUtils(gmsol_solana_utils::Error),
    /// Anchor Lang Error.
    #[error("anchor-lang: {0}")]
    AnchorLang(Box<AnchorLangError>),
    /// Anchor Error.
    #[error("anchor: {0:#?}")]
    Anchor(AnchorError),
    /// Model Error.
    #[error("model: {0}")]
    Model(#[from] gmsol_model::Error),
    /// Base64 decode error.
    #[error("base64-decode: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    /// Bincode error.
    #[cfg(feature = "bincode")]
    #[error("bincode: {0}")]
    Bincode(#[from] bincode::Error),
    /// Custom error.
    #[error("custom: {0}")]
    Custom(String),
    /// Transport error.
    #[error("transport: {0}")]
    Transport(String),
    /// Market Graph Errors
    #[cfg(feature = "market-graph")]
    #[error("market-graph: {0}")]
    MarketGraph(#[from] crate::market_graph::error::MarketGraphError),
    /// Parse Pubkey Error.
    #[error("parse pubkey error: {0}")]
    ParsePubkey(#[from] solana_sdk::pubkey::ParsePubkeyError),
    /// Pubsub client closed.
    #[cfg(feature = "client")]
    #[error("pubsub: closed")]
    PubsubClosed,
    /// Not found error.
    #[error("not found")]
    NotFound,
    /// Decode error.
    #[cfg(feature = "decode")]
    #[error("decode: {0}")]
    Decode(#[from] gmsol_decode::DecodeError),
    /// Error from [`gmsol_programs`].
    #[error("programs: {0}")]
    Programs(#[from] gmsol_programs::Error),
    /// Reqwest error.
    #[cfg(feature = "reqwest")]
    #[error("reqwest: {0}")]
    Reqwest(#[from] reqwest::Error),
    /// Json error.
    #[cfg(feature = "serde_json")]
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    /// Create a custom error.
    pub fn custom(msg: impl ToString) -> Self {
        Self::Custom(msg.to_string())
    }

    /// Create a transport error.
    pub fn transport(msg: impl ToString) -> Self {
        Self::Transport(msg.to_string())
    }

    /// Anchor Error Code.
    pub fn anchor_error_code(&self) -> Option<u32> {
        let Self::Anchor(error) = self else {
            return None;
        };
        Some(error.error_code_number)
    }
}

impl From<AnchorLangError> for Error {
    fn from(value: AnchorLangError) -> Self {
        Self::AnchorLang(Box::new(value))
    }
}

#[cfg(feature = "wasm-bindgen")]
impl From<Error> for wasm_bindgen::JsValue {
    fn from(value: Error) -> Self {
        Self::from_str(&value.to_string())
    }
}

/// Anchor Error with owned source.
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
pub enum ErrorOrigin {
    /// Source.
    Source(String, u32),
    /// Account.
    AccountName(String),
}

#[cfg(feature = "solana-client")]
fn handle_solana_client_error(error: &solana_client::client_error::ClientError) -> Option<Error> {
    use solana_client::{
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

impl From<gmsol_solana_utils::Error> for Error {
    fn from(value: gmsol_solana_utils::Error) -> Self {
        match value {
            #[cfg(feature = "solana-client")]
            gmsol_solana_utils::Error::Client(err) => match handle_solana_client_error(&err) {
                Some(err) => err,
                None => Self::SolanaUtils(err.into()),
            },
            err => Self::SolanaUtils(err),
        }
    }
}

impl<T> From<(T, gmsol_solana_utils::Error)> for Error {
    fn from((_, err): (T, gmsol_solana_utils::Error)) -> Self {
        Self::from(err)
    }
}
