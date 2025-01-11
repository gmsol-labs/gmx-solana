use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use crate::states::gt::GtState;

use super::Event;

/// GT updated event.
#[event]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(InitSpace)]
pub struct GtUpdated {
    /// Initiator.
    pub initiator: Pubkey,
    /// Whether the GT is minted or burned.
    pub is_minted: bool,
    /// Whether the GT is reward.
    pub is_reward: bool,
    /// Delta.
    pub delta: u64,
    /// Minting cost.
    pub minting_cost: u128,
    /// Total minted.
    pub total_minted: u64,
    /// Grow steps.
    pub grow_steps: u64,
    /// Latest supply.
    pub supply: u64,
    /// Vault.
    pub vault: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 64],
}

impl gmsol_utils::InitSpace for GtUpdated {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for GtUpdated {}

impl GtUpdated {
    fn new(
        initiator: Pubkey,
        minting_cost: Option<u128>,
        is_reward: bool,
        delta: u64,
        state: &GtState,
    ) -> Self {
        Self {
            initiator,
            is_minted: minting_cost.is_some(),
            is_reward,
            delta,
            minting_cost: minting_cost.unwrap_or(state.minting_cost()),
            total_minted: state.total_minted(),
            grow_steps: state.grow_steps(),
            supply: state.supply(),
            vault: state.gt_vault(),
            reserved: [0; 64],
        }
    }

    /// Create a new rewarded event.
    pub fn rewarded(initiator: Pubkey, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, Some(state.minting_cost()), true, amount, state)
    }

    /// Create a new minted event.
    pub fn minted(initiator: Pubkey, minting_cost: u128, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, Some(minting_cost), false, amount, state)
    }

    /// Create a new burned event.
    pub fn burned(initiator: Pubkey, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, None, false, amount, state)
    }
}
