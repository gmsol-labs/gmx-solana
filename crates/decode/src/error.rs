/// Decode Error.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Custom Error.
    #[error("custom: {0}")]
    Custom(String),
    /// Invalid Type.
    #[error("invalid type: {0}")]
    InvalidType(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anchor Error.
    #[error(transparent)]
    Anchor(#[from] anchor_lang::prelude::Error),
}

impl DecodeError {
    /// Create a custom error.
    pub fn custom(msg: impl ToString) -> Self {
        Self::Custom(msg.to_string())
    }
}
