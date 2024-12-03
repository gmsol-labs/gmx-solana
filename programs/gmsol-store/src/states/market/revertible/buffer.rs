use anchor_lang::prelude::*;
use gmsol_model::PoolKind;
use strum::IntoEnumIterator;

use crate::states::{
    market::{Clocks, Pool, State},
    OtherState, PoolStorage,
};

#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub(crate) struct RevertibleBuffer {
    rev: u64,
    padding: [u8; 8],
    state: State,
}

impl RevertibleBuffer {
    pub(crate) fn init(&mut self) {
        self.rev = 1;
    }

    pub(super) fn pool<'a>(&'a self, kind: PoolKind, storage: &'a State) -> Option<&'a Pool> {
        let pool_storage = self.state.pools.get(kind)?;
        Some(
            pool_storage
                .cache_get_with(self.rev, || storage.pools.get(kind).expect("must exist"))
                .pool(),
        )
    }

    pub(super) fn pool_mut(&mut self, kind: PoolKind, storage: &State) -> Option<&mut Pool> {
        let pool_storage = self.state.pools.get_mut(kind)?;
        Some(
            pool_storage
                .cache_get_mut_with(self.rev, || *storage.pools.get(kind).expect("must exist"))
                .pool_mut(),
        )
    }

    pub(super) fn clocks<'a>(&'a self, storage: &'a State) -> &'a Clocks {
        self.state
            .clocks
            .cache_get_with(self.rev, || &storage.clocks)
    }

    pub(super) fn clocks_mut(&mut self, storage: &State) -> &mut Clocks {
        self.state
            .clocks
            .cache_get_mut_with(self.rev, || storage.clocks)
    }

    pub(super) fn other<'a>(&'a self, storage: &'a State) -> &'a OtherState {
        self.state.other.cache_get_with(self.rev, || &storage.other)
    }

    pub(super) fn other_mut(&mut self, storage: &State) -> &mut OtherState {
        self.state
            .other
            .cache_get_mut_with(self.rev, || storage.other)
    }

    pub(super) fn start_revertible_operation(&mut self) {
        self.rev = self.rev.checked_add(1).expect("rev overflow");
    }

    pub(super) fn commit_to_storage(&mut self, storage: &mut State) {
        let state = &mut self.state;
        let rev = self.rev;

        // Commit pools.
        for kind in PoolKind::iter() {
            let Some(pool) = state.pools.get_mut(kind) else {
                continue;
            };
            if pool.is_dirty(rev) {
                let target = storage.pools.get_mut(kind).expect("must exist");
                *target = *pool;
            }
        }

        // Commit clocks.
        if state.clocks.is_dirty(rev) {
            storage.clocks = state.clocks;
        }

        // Commit other state.
        if state.other.is_dirty(rev) {
            storage.other = state.other;
        }
    }
}

pub(super) trait Cache {
    fn is_dirty(&self, rev: u64) -> bool;

    fn rev(&self) -> u64;

    fn set_rev(&mut self, rev: u64) -> u64;

    fn cache_get_with<'a>(&'a self, rev: u64, f: impl FnOnce() -> &'a Self) -> &'a Self {
        if self.is_dirty(rev) {
            self
        } else {
            f()
        }
    }

    fn cache_get_mut_with(&mut self, rev: u64, f: impl FnOnce() -> Self) -> &mut Self
    where
        Self: Sized,
    {
        if !self.is_dirty(rev) {
            *self = f();
            self.set_rev(rev);
        }
        self
    }
}

macro_rules! impl_cache {
    ($cache:ty) => {
        impl Cache for $cache {
            fn is_dirty(&self, rev: u64) -> bool {
                self.rev() == rev
            }

            fn rev(&self) -> u64 {
                self.rev
            }

            fn set_rev(&mut self, mut rev: u64) -> u64 {
                std::mem::swap(&mut rev, &mut self.rev);
                rev
            }
        }
    };
}

impl_cache!(Clocks);
impl_cache!(PoolStorage);
impl_cache!(OtherState);
