/// Simulator.
pub mod simulator;

/// Order simulation.
pub mod order;

/// Liquidity (GM) exact simulation.
pub mod lp;

pub use lp::{
    simulate_gm_deposit_exact, simulate_gm_withdrawal_exact, GmDepositExactOutput,
    GmWithdrawalExactOutput,
};
pub use simulator::{SimulationOptions, Simulator, SwapOutput, TokenState};
