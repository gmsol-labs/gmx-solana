#![deny(missing_docs)]
#![deny(unreachable_pub)]
#![warn(clippy::arithmetic_side_effects)]

//! A Rust implementation of GMX V2 Model.

/// Pool.
pub mod pool;

/// Market.
pub mod market;

/// Bank.
pub mod bank;

/// Clock.
pub mod clock;

/// Position.
pub mod position;

/// Price.
pub mod price;

/// Market params.
pub mod params;

/// Actions.
pub mod action;

/// Error type.
pub mod error;

/// Number utils.
pub mod num;

/// Fixed-point decimal type.
pub mod fixed;

/// Utils.
pub mod utils;

/// Utils for testing.
#[cfg(any(test, feature = "test"))]
pub mod test;

pub use action::MarketAction;
pub use bank::Bank;
pub use clock::ClockKind;
pub use error::Error;
pub use market::{
    BaseMarket, BaseMarketExt, BaseMarketMut, BaseMarketMutExt, BorrowingFeeMarket,
    BorrowingFeeMarketExt, LiquidityMarket, LiquidityMarketExt, LiquidityMarketMut,
    LiquidityMarketMutExt, PerpMarket, PerpMarketExt, PerpMarketMut, PerpMarketMutExt,
    PnlFactorKind, PositionImpactMarket, PositionImpactMarketExt, PositionImpactMarketMut,
    PositionImpactMarketMutExt, SwapMarket, SwapMarketExt, SwapMarketMut, SwapMarketMutExt,
};
pub use pool::{Balance, BalanceExt, Delta, Pool, PoolExt, PoolKind};
pub use position::{
    Position, PositionExt, PositionMut, PositionMutExt, PositionState, PositionStateExt,
    PositionStateMut,
};

/// Alias for result.
pub type Result<T> = std::result::Result<T, Error>;
