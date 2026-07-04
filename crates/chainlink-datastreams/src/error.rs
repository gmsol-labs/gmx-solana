/// Error type for this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Negative price.
    #[error("negative price: {0}")]
    NegativePrice(&'static str),
    /// Invalid data range.
    #[error("invalid data range: {0}")]
    InvalidRange(&'static str),
    /// Overflow.
    #[error("overflow: {0}")]
    Overflow(&'static str),
}
