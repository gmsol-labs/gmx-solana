use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use gmsol_utils::InitSpace;

use crate::states::common::action::ActionState;

use super::Event;

/// Shift removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct ShiftRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Shift.
    pub shift: Pubkey,
    /// Market token.
    pub market_token: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Final state.
    pub state: ActionState,
    /// Reason.
    #[max_len(32)]
    pub reason: String,
}

impl ShiftRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        shift: Pubkey,
        market_token: Pubkey,
        owner: Pubkey,
        state: ActionState,
        reason: impl ToString,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            id,
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store,
            shift,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for ShiftRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for ShiftRemoved {}
