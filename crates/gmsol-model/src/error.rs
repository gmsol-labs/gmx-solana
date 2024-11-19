use crate::{position::LiquidatableReason, ClockKind, PnlFactorKind, PoolKind};

/// Error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unimplemented.
    #[error("unimplemented")]
    Unimplemented,
    /// Invalid Argument.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    /// Empty deposit.
    #[error("empty deposit")]
    EmptyDeposit,
    /// Empty withdrawal.
    #[error("empty withdrawal")]
    EmptyWithdrawal,
    /// Empty swap.
    #[error("empty swap")]
    EmptySwap,
    /// Invalid prices.
    #[error("invalid prices")]
    InvalidPrices,
    /// Unknown computation error.
    #[error("unknown computation error: {0}")]
    Computation(&'static str),
    ///  Computation error in pool
    #[error("computation in `{0:?}` pool error: {1}")]
    PoolComputation(PoolKind, &'static str),
    /// Power computation error.
    #[error("pow computation error")]
    PowComputation,
    /// Overflow.
    #[error("overflow")]
    Overflow,
    /// Divided by zero.
    #[error("divided by zero")]
    DividedByZero,
    /// Invalid pool value.
    #[error("invalid pool value {0}")]
    InvalidPoolValue(&'static str),
    /// Convert error.
    #[error("convert value error")]
    Convert,
    /// Anchor error.
    #[cfg(feature = "solana")]
    #[error(transparent)]
    Solana(#[from] anchor_lang::prelude::Error),
    /// Build params error.
    #[error("build params: {0}")]
    BuildParams(&'static str),
    /// Missing pool of kind.
    #[error("missing pool of kind: {0:?}")]
    MissingPoolKind(PoolKind),
    /// Missing clock of kind.
    #[error("missing clock of kind: {0:?}")]
    MissingClockKind(ClockKind),
    /// Mint receiver not set.
    #[error("mint receiver not set")]
    MintReceiverNotSet,
    /// Withdrawal vault not set.
    #[error("withdrawal vault not set")]
    WithdrawalVaultNotSet,
    /// Insufficient funds to pay for cost.
    #[error("insufficient funds to pay for costs")]
    InsufficientFundsToPayForCosts,
    /// Invalid position state.
    #[error("invalid position state: {0}")]
    InvalidPosition(&'static str),
    /// Liquidatable Position.
    #[error("liquidatable position: {0}")]
    Liquidatable(LiquidatableReason),
    /// Not liquidatable.
    #[error("not liquidatable")]
    NotLiquidatable,
    /// Unable to get borrowing factor for empty pool value.
    #[error("unable to get borrowing factor for empty pool value")]
    UnableToGetBorrowingFactorEmptyPoolValue,
    /// Insufficient reserve.
    #[error("insufficient reserve")]
    InsufficientReserve,
    /// Insufficient reserve for open interest.
    #[error("insufficient reserve for open interest")]
    InsufficientReserveForOpenInterest,
    /// Pnl Factor Exceeded.
    #[error("pnl factor ({0:?}) exceeded {1}")]
    PnlFactorExceeded(PnlFactorKind, &'static str),
    /// Max pool amount exceeded.
    #[error("max pool amount exceeded: {0}")]
    MaxPoolAmountExceeded(&'static str),
    /// Max pool value for deposit exceeded.
    #[error("max pool value exceeded: {0}")]
    MaxPoolValueExceeded(&'static str),
    /// Max open interest exceeded.
    #[error("max open interest exceeded")]
    MaxOpenInterestExceeded,
    /// Invalid token balance.
    #[error("invalid token balance: {0}, expected={1}, balance={2}")]
    InvalidTokenBalance(&'static str, String, String),
}
