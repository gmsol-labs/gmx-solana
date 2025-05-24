use anchor_lang::prelude::*;
use borsh::BorshSerialize;
use gmsol_model::action::deposit::DepositReport;
use gmsol_utils::InitSpace;

use crate::states::common::action::ActionState;

use super::Event;

/// Deposit Created Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DepositCreated {
    /// Event time.
    pub ts: i64,
    /// Store account.
    pub store: Pubkey,
    /// Deposit account.
    pub deposit: Pubkey,
}

impl DepositCreated {
    pub(crate) fn new(store: Pubkey, deposit: Pubkey) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            deposit,
        })
    }
}

/// Deposit executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DepositExecuted {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: DepositReport<u128, i128>,
}

impl gmsol_utils::InitSpace for DepositExecuted {
    const INIT_SPACE: usize = 8 + 32 + DepositReport::<u128, i128>::INIT_SPACE;
}

impl Event for DepositExecuted {}

impl DepositExecuted {
    pub(crate) fn from_report(
        rev: u64,
        market_token: Pubkey,
        report: DepositReport<u128, i128>,
    ) -> Self {
        Self {
            rev,
            market_token,
            report,
        }
    }
}

/// Deposit removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct DepositRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Deposit.
    pub deposit: Pubkey,
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

impl DepositRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        deposit: Pubkey,
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
            deposit,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for DepositRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for DepositRemoved {}
