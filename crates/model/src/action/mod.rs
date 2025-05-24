/// Deposit.
pub mod deposit;

/// Withdraw.
pub mod withdraw;

/// Swap.
pub mod swap;

/// Increase Position.
pub mod increase_position;

/// Decrease Position.
pub mod decrease_position;

/// Distribute position impact.
pub mod distribute_position_impact;

/// Update borrowing state.
pub mod update_borrowing_state;

/// Update funding state.
pub mod update_funding_state;

/// Market Action.
#[must_use = "actions do nothing unless you `execute` them"]
pub trait MarketAction {
    /// The type of the execution report of the action.
    type Report;

    /// Execute.
    fn execute(self) -> crate::Result<Self::Report>;
}
