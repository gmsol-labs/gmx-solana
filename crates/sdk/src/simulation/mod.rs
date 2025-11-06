/// Simulator.
pub mod simulator;

/// Order simulation.
pub mod order;

/// Deposit simulation.
pub mod deposit;

/// Withdrawal simulation.
pub mod withdrawal;

/// GLV deposit simulation.
pub mod glv_deposit;

/// GLV withdrawal simulation.
pub mod glv_withdrawal;

/// Liquidity (GM) exact simulation.
pub mod lp;

pub use lp::{
    simulate_gm_deposit_exact, simulate_gm_withdrawal_exact, GmDepositExactOutput,
    GmWithdrawalExactOutput,
};
pub use simulator::{SimulationOptions, Simulator, SwapOutput, TokenState};
