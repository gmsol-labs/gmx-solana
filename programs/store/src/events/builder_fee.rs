use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use super::Event;

/// An event indicating that a builder fee has been settled.
///
/// Emitted only when settlement actually transfers a non-zero amount; the
/// no-op path (a recorded amount of zero) emits nothing.
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
    /// The order the settled fee was accrued on.
    pub order: Pubkey,
    /// The builder's User Account.
    pub builder: Pubkey,
    /// The token mint the fee is denominated in.
    pub token: Pubkey,
    /// The builder fee amount recorded on the order before settlement.
    pub recorded_amount: u64,
    /// The amount actually transferred, clamped to the escrow's balance.
    /// A value below `recorded_amount` signals a broken invariant, since
    /// the escrow is expected to always cover the recorded amount.
    pub settled_amount: u64,
}

impl BuilderFeeSettled {
    pub(crate) fn new(
        store: &Pubkey,
        order: &Pubkey,
        builder: &Pubkey,
        token: &Pubkey,
        recorded_amount: u64,
        settled_amount: u64,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            order: *order,
            builder: *builder,
            token: *token,
            recorded_amount,
            settled_amount,
        })
    }
}

impl InitSpace for BuilderFeeSettled {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeSettled {}
