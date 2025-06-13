mod buffer;

/// Revertible Market.
pub mod market;

/// Revertible Swap Market.
pub mod swap_market;

/// Revertible Liquidity Market.
pub mod liquidity_market;

/// Revertible Position.
pub mod revertible_position;

/// Revertible Virtual Inventory.
pub mod revertible_virtual_inventory;

pub use self::{
    liquidity_market::RevertibleLiquidityMarket, market::RevertibleMarket,
    revertible_position::RevertiblePosition,
};

pub(crate) use self::{
    buffer::{RevertibleBuffer, RevertiblePoolBuffer},
    revertible_virtual_inventory::RevertibleVirtualInventories,
};

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
