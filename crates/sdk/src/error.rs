pub(crate) use gmsol_programs::anchor_lang::prelude::Error as AnchorLangError;

/// SDK Error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error from [`gmsol-solana-utils`].
    #[error("utils: {0}")]
    SolanaUtils(#[from] gmsol_solana_utils::Error),
    /// Anchor Error.
    #[error("anchor: {0}")]
    Anchor(Box<AnchorLangError>),
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
    /// Unknown error.
    #[error("unknown: {0}")]
    Unknown(String),
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
    /// Create an unknown error.
    pub fn unknown(msg: impl ToString) -> Self {
        Self::Unknown(msg.to_string())
    }

    /// Create a transport error.
    pub fn transport(msg: impl ToString) -> Self {
        Self::Transport(msg.to_string())
    }
}

impl From<AnchorLangError> for Error {
    fn from(value: AnchorLangError) -> Self {
        Self::Anchor(Box::new(value))
    }
}

#[cfg(feature = "wasm-bindgen")]
impl From<Error> for wasm_bindgen::JsValue {
    fn from(value: Error) -> Self {
        Self::from_str(&value.to_string())
    }
}
