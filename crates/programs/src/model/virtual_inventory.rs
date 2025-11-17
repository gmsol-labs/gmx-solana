use std::ops::Deref;
use std::sync::Arc;

use crate::gmsol_store::{accounts::VirtualInventory, types::Pool};

/// Virtual Inventory Model.
///
/// An off-chain encapsulation of the on-chain [`VirtualInventory`] account.
/// This encapsulation uses Copy-On-Write (COW) semantics via [`Arc`] to allow
/// efficient cloning while enabling mutations when needed.
#[derive(Debug, Clone)]
pub struct VirtualInventoryModel {
    virtual_inventory: Arc<VirtualInventory>,
}

impl Deref for VirtualInventoryModel {
    type Target = VirtualInventory;

    fn deref(&self) -> &Self::Target {
        &self.virtual_inventory
    }
}

impl VirtualInventoryModel {
    /// Create from parts.
    pub fn from_parts(virtual_inventory: Arc<VirtualInventory>) -> Self {
        Self { virtual_inventory }
    }

    /// Get the pool from the virtual inventory.
    ///
    /// This returns a reference to the [`Pool`] stored in the
    /// [`PoolStorage`] of the virtual inventory.
    pub fn pool(&self) -> &Pool {
        &self.virtual_inventory.pool.pool
    }

    /// Get a mutable reference to the pool.
    ///
    /// This will trigger Copy-On-Write if the virtual inventory
    /// is shared with other instances.
    pub fn pool_mut(&mut self) -> &mut Pool {
        let vi = self.make_virtual_inventory_mut();
        // Access the pool field directly through the PoolStorage
        &mut vi.pool.pool
    }

    /// Get a mutable reference to the virtual inventory.
    ///
    /// This will trigger Copy-On-Write if the virtual inventory
    /// is shared with other instances.
    fn make_virtual_inventory_mut(&mut self) -> &mut VirtualInventory {
        Arc::make_mut(&mut self.virtual_inventory)
    }
}
