use anchor_lang::prelude::*;

use gmsol_utils::InitSpace;

use super::Event;

/// An event indicating that a builder fee has been settled.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct BuilderFeeSettled {
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Order.
    pub order: Pubkey,
    /// The builder's user account.
    pub builder: Pubkey,
    /// The token the fee is denominated in.
    pub token: Pubkey,
    /// The settled fee amount.
    pub amount: u64,
}

impl BuilderFeeSettled {
    pub(crate) fn new(
        store: &Pubkey,
        order: &Pubkey,
        builder: &Pubkey,
        token: &Pubkey,
        amount: u64,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            order: *order,
            builder: *builder,
            token: *token,
            amount,
        })
    }
}

impl InitSpace for BuilderFeeSettled {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeSettled {}

/// An event indicating that builder fees have been claimed.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct BuilderFeeClaimed {
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// The builder's user account.
    pub builder: Pubkey,
    /// The token claimed.
    pub token: Pubkey,
    /// The destination token account.
    pub receiver: Pubkey,
    /// The claimed amount.
    pub amount: u64,
}

impl BuilderFeeClaimed {
    pub(crate) fn new(
        store: &Pubkey,
        builder: &Pubkey,
        token: &Pubkey,
        receiver: &Pubkey,
        amount: u64,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            builder: *builder,
            token: *token,
            receiver: *receiver,
            amount,
        })
    }
}

impl InitSpace for BuilderFeeClaimed {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeClaimed {}
