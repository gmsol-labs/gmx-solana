use anchor_lang::prelude::*;
use gmsol_model::PoolKind;
use strum::IntoEnumIterator;

use crate::{
    events::{EventEmitter, MarketStateUpdatedRef},
    states::{
        market::{Clocks, Pool, State},
        OtherState, PoolStorage,
    },
};

use super::Revision;

#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    pub(super) fn rev(&self) -> u64 {
        self.rev
    }

    pub(super) fn start_revertible_operation(&mut self) {
        self.rev = self.rev.checked_add(1).expect("rev overflow");
    }

    pub(super) fn commit_to_storage(
        &mut self,
        storage: &mut State,
        market_token: &Pubkey,
        event_emitter: &EventEmitter,
    ) {
        let state = &self.state;
        let rev = self.rev;

        let mut updated_pool_kinds = Vec::new();

        // Commit pools.
        for kind in PoolKind::iter() {
            let Some(pool) = state.pools.get(kind) else {
                continue;
            };
            if pool.is_dirty(rev) {
                let target = storage.pools.get_mut(kind).expect("must exist");
                *target = *pool;
                updated_pool_kinds.push(kind);
            }
        }

        // Commit clocks.
        let is_clocks_dirty = state.clocks.is_dirty(rev);
        if is_clocks_dirty {
            storage.clocks = state.clocks;
        }

        // Commit other state.
        let is_other_dirty = state.other.is_dirty(rev);
        if is_other_dirty {
            storage.other = state.other;
        }

        let updated_pools = updated_pool_kinds
            .iter()
            .map(|kind| state.pools.get(*kind).expect("must exist").pool())
            .collect();

        let event = MarketStateUpdatedRef::new(
            self.rev(),
            *market_token,
            updated_pool_kinds,
            updated_pools,
            is_clocks_dirty.then_some(&state.clocks),
            is_other_dirty.then_some(&state.other),
        );

        event_emitter
            .emit_cpi_with_space(&event, event.space())
            .map_err(|err| {
                msg!("{}", err);
            })
            .expect("emit event error");
    }
}

pub(super) trait Cache: Revision {
    fn is_dirty(&self, rev: u64) -> bool;

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
        impl Revision for $cache {
            fn rev(&self) -> u64 {
                self.rev
            }
        }

        impl Cache for $cache {
            fn is_dirty(&self, rev: u64) -> bool {
                self.rev() == rev
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
