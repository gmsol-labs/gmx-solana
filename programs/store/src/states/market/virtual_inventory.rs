use anchor_lang::prelude::*;
use gmsol_model::{Delta, Pool};
use gmsol_utils::market::{VirtualInventoryFlag, MAX_VIRTUAL_INVENTORY_FLAGS};

use crate::{CoreError, ModelError};

use super::{pool::PoolStorage, revertible::RevertiblePoolBuffer};

/// The seed of virtual inventory for swaps accounts.
#[constant]
pub const VIRTUAL_INVENTORY_FOR_SWAPS_SEED: &[u8] = b"vi_for_swaps";

/// The seed of virtual inventory for positions accounts.
#[constant]
pub const VIRTUAL_INVENTORY_FOR_POSITIONS_SEED: &[u8] = b"vi_for_positions";

gmsol_utils::flags!(VirtualInventoryFlag, MAX_VIRTUAL_INVENTORY_FLAGS, u8);

/// General purpose virtual inventory.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VirtualInventory {
    version: u8,
    pub(crate) bump: u8,
    flags: VirtualInventoryFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    padding_0: [u8; 5],
    ref_count: u32,
    pub(crate) index: u32,
    rev: u64,
    padding_1: [u8; 8],
    pub(crate) store: Pubkey,
    pool: PoolStorage,
    buffer: RevertiblePoolBuffer,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved_0: [u8; 128],
}

impl VirtualInventory {
    pub(crate) fn init(&mut self, bump: u8, index: u32, store: Pubkey) {
        self.bump = bump;
        self.index = index;
        self.store = store;
    }

    pub(crate) fn is_disabled(&self) -> bool {
        self.flags.get_flag(VirtualInventoryFlag::Disabled)
    }

    pub(crate) fn disable(&mut self) -> Result<()> {
        require!(!self.is_disabled(), CoreError::PreconditionsAreNotMet);
        self.flags.set_flag(VirtualInventoryFlag::Disabled, true);
        Ok(())
    }

    pub(crate) fn ref_count(&self) -> u32 {
        self.ref_count
    }

    pub(crate) fn pool(&self) -> &PoolStorage {
        &self.pool
    }

    pub(crate) fn buffer(&self) -> &RevertiblePoolBuffer {
        &self.buffer
    }

    pub(crate) fn buffer_mut(&mut self) -> &mut RevertiblePoolBuffer {
        &mut self.buffer
    }

    pub(crate) fn split(&mut self) -> (&mut RevertiblePoolBuffer, &mut PoolStorage) {
        (&mut self.buffer, &mut self.pool)
    }

    /// Increases the `ref_count` and applys the delta to the pool.
    ///
    /// # CHECK
    /// - It can only be called once the market is associated, and cannot be called again.
    pub(crate) fn join_unchecked(&mut self, delta: Delta<&i128>) -> Result<()> {
        let pool = self.pool.pool_mut();
        let next_pool = pool.checked_apply_delta(delta).map_err(ModelError::from)?;
        self.ref_count = self
            .ref_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::IndexOverflow))?;
        *pool = next_pool;
        Ok(())
    }

    /// Decreases the `ref_count` and applys the delta to the pool.
    ///
    /// # CHECK
    /// - It can only be called once the market is de-associated, and cannot be called again.
    pub(crate) fn leave_unchecked(&mut self, delta: Delta<&i128>) -> Result<()> {
        let pool = self.pool.pool_mut();
        let next_pool = pool.checked_apply_delta(delta).map_err(ModelError::from)?;
        self.ref_count = self
            .ref_count
            .checked_sub(1)
            .ok_or_else(|| error!(CoreError::PreconditionsAreNotMet))?;
        *pool = next_pool;
        Ok(())
    }

    /// Cancel the pool amounts.
    ///
    /// # CHECK
    /// The cancel amounts operation must be well-defined for this
    /// virtual inventory.
    /// For example, it is a virtual inventory for positions.
    pub(crate) fn cancel_amounts_unchecked(&mut self) -> Result<()> {
        *self.pool.pool_mut() = self
            .pool
            .pool()
            .checked_cancel_amounts()
            .map_err(ModelError::from)?;
        Ok(())
    }
}

impl gmsol_utils::InitSpace for VirtualInventory {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}
