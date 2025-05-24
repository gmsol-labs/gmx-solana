use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use gmsol_model::action::withdraw::WithdrawReport;
use gmsol_utils::InitSpace;

use crate::states::common::action::ActionState;

use super::Event;

/// Withdrawal created event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct WithdrawalCreated {
    /// Event time.
    pub ts: i64,
    /// Store account.
    pub store: Pubkey,
    /// Withdrawal account.
    pub withdrawal: Pubkey,
}

impl WithdrawalCreated {
    pub(crate) fn new(store: Pubkey, withdrawal: Pubkey) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            withdrawal,
        })
    }
}

/// Withdrawal executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct WithdrawalExecuted {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: WithdrawReport<u128>,
}

impl gmsol_utils::InitSpace for WithdrawalExecuted {
    const INIT_SPACE: usize = 8 + 32 + WithdrawReport::<u128>::INIT_SPACE;
}

impl Event for WithdrawalExecuted {}

impl WithdrawalExecuted {
    pub(crate) fn from_report(
        rev: u64,
        market_token: Pubkey,
        report: WithdrawReport<u128>,
    ) -> Self {
        Self {
            rev,
            market_token,
            report,
        }
    }
}

/// Withdrawal removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct WithdrawalRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Withdrawal.
    pub withdrawal: Pubkey,
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

impl InitSpace for WithdrawalRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for WithdrawalRemoved {}

impl WithdrawalRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        withdrawal: Pubkey,
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
            withdrawal,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}
