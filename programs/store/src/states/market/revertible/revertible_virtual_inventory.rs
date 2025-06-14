use std::{
    cell::{Ref, RefMut},
    collections::BTreeMap,
    ops::Deref,
};

use anchor_lang::prelude::*;

use crate::{
    debug_msg,
    ops::market::VirtualInventoryLoaders,
    states::market::{pool::Pool, virtual_inventory::VirtualInventory},
};

use super::Revertible;

/// Revertible [`VirtualInventory`].
pub struct RevertibleVirtualInventory<'info> {
    virtual_inventory: AccountLoader<'info, VirtualInventory>,
}

impl<'info> RevertibleVirtualInventory<'info> {
    pub(crate) fn new(virtual_inventory: &AccountLoader<'info, VirtualInventory>) -> Result<Self> {
        virtual_inventory
            .load_mut()?
            .buffer_mut()
            .start_revertible_operation();
        Ok(Self {
            virtual_inventory: virtual_inventory.clone(),
        })
    }

    pub(crate) fn pool(&self) -> Result<Ref<'_, Pool>> {
        Ok(Ref::map(self.virtual_inventory.load()?, |vi| {
            vi.buffer().pool(vi.pool())
        }))
    }

    pub(crate) fn pool_mut(&self) -> Result<RefMut<'_, Pool>> {
        Ok(RefMut::map(self.virtual_inventory.load_mut()?, |vi| {
            let (buffer, pool) = vi.split();
            buffer.pool_mut(pool)
        }))
    }

    pub(crate) fn is_disabled(&self) -> Result<bool> {
        Ok(self.virtual_inventory.load()?.is_disabled())
    }
}

impl Revertible for RevertibleVirtualInventory<'_> {
    fn commit(self) {
        let mut vi = self.virtual_inventory.load_mut().expect("must success");
        let (buffer, storage) = vi.split();
        buffer.commit_to_storage(storage);
        debug_msg!(
            "[VI committed]: {},{}",
            storage.pool().long_token_amount,
            storage.pool().short_token_amount,
        )
    }
}

/// A map of [`RevertibleVirtualInventory`].
pub(crate) struct RevertibleVirtualInventories<'info> {
    map: BTreeMap<&'info Pubkey, RevertibleVirtualInventory<'info>>,
}

impl<'info> RevertibleVirtualInventories<'info> {
    pub(crate) fn from_loaders(loaders: &VirtualInventoryLoaders<'info>) -> Result<Self> {
        let map = loaders
            .iter()
            .map(|(key, loader)| Ok((*key, RevertibleVirtualInventory::new(loader)?)))
            .collect::<Result<_>>()?;
        Ok(Self { map })
    }
}

impl<'info> Deref for RevertibleVirtualInventories<'info> {
    type Target = BTreeMap<&'info Pubkey, RevertibleVirtualInventory<'info>>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl Revertible for RevertibleVirtualInventories<'_> {
    fn commit(self) {
        for virtual_inventory in self.map.into_values() {
            virtual_inventory.commit();
        }
    }
}
