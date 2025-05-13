/// General purpose error type for this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Custom.
    #[error("custom: {0}")]
    Custom(String),
}

impl Error {
    /// Create a custom error.
    pub fn custom(msg: impl ToString) -> Self {
        Self::Custom(msg.to_string())
    }
}
