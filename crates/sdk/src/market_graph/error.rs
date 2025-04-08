/// Market Graph Error.
#[derive(Debug, thiserror::Error)]
pub enum MarketGraphError {
    /// Negative Cycle.
    #[error("negative cycle")]
    NegativeCycle,
}
