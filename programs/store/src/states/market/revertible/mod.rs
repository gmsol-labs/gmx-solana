mod buffer;

/// Revertible Market.
pub mod market;

/// Revertible Swap Market.
pub mod swap_market;

/// Revertible Liquidity Market.
pub mod liquidity_market;

/// Revertible Position.
pub mod revertible_position;

pub use self::{
    liquidity_market::RevertibleLiquidityMarket, market::RevertibleMarket,
    revertible_position::RevertiblePosition,
};

pub(super) use self::buffer::RevertibleBuffer;

/// Revertible type.
pub trait Revertible {
    /// Commit the changes.
    ///
    /// ## Panic
    /// - Should panic if the commitment cannot be done.
    fn commit(self);
}

/// Type that has a revision.
pub trait Revision {
    /// Get the revision.
    fn rev(&self) -> u64;
}
