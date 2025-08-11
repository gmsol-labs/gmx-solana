/// Order execution simulation.
pub mod order;

/// Options for simulation.
#[derive(Debug, Default, Clone)]
pub struct SimulationOptions {
    /// Whether to prefer swap in token update.
    pub prefer_swap_in_token_update: bool,
}
