use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use super::Event;

/// The complete GT delegation state after an owner-authorized change.
#[event]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(InitSpace)]
pub struct GtDelegationUpdated {
    /// The changed User Account.
    pub user: Pubkey,
    /// The owner of the User Account.
    pub owner: Pubkey,
    /// The current delegate, or the zero address when cleared.
    pub delegate: Pubkey,
    /// The current remaining allowance, in raw GT units.
    pub allowance: u64,
}

impl gmsol_utils::InitSpace for GtDelegationUpdated {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for GtDelegationUpdated {}

/// Builder fee factor set event.
#[event]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(InitSpace)]
pub struct BuilderFeeFactorSet {
    /// The User Account whose builder fee factor was set.
    pub user: Pubkey,
    /// The owner of the User Account.
    pub owner: Pubkey,
    /// The builder fee factor before this update.
    pub previous_factor: u128,
    /// The builder fee factor after this update.
    pub factor: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 64],
}

impl gmsol_utils::InitSpace for BuilderFeeFactorSet {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeFactorSet {}

impl BuilderFeeFactorSet {
    pub(crate) fn new(user: Pubkey, owner: Pubkey, previous_factor: u128, factor: u128) -> Self {
        Self {
            user,
            owner,
            previous_factor,
            factor,
            reserved: [0; 64],
        }
    }
}
