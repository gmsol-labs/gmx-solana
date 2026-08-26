use anchor_lang::prelude::*;
use borsh::BorshSerialize;
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

/// Builder fee set event.
///
/// Emitted whenever a builder and its fee factor are checkpointed onto an
/// order. This is the only event carrying the order-to-builder mapping: the
/// charge emitted at execution reports amounts, not who they are owed to, so an
/// indexer that misses this event cannot attribute a fee.
///
/// Re-checkpointing emits again, with the replaced values in the `previous_*`
/// fields, so the full history of an order's builder is reconstructible from
/// this event alone.
#[event]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(InitSpace)]
pub struct BuilderFeeSet {
    /// The store the order belongs to.
    pub store: Pubkey,
    /// The order the builder fee was checkpointed onto.
    pub order: Pubkey,
    /// The builder's User Account.
    pub builder: Pubkey,
    /// The checkpointed builder fee factor.
    pub factor: u128,
    /// The builder attached before this call.
    ///
    /// The default (zero) [`Pubkey`] means the order had no builder.
    pub previous_builder: Pubkey,
    /// The builder fee factor checkpointed before this call.
    pub previous_factor: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 64],
}

impl gmsol_utils::InitSpace for BuilderFeeSet {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeSet {}

impl BuilderFeeSet {
    pub(crate) fn new(
        store: Pubkey,
        order: Pubkey,
        builder: Pubkey,
        factor: u128,
        previous_builder: Pubkey,
        previous_factor: u128,
    ) -> Self {
        Self {
            store,
            order,
            builder,
            factor,
            previous_builder,
            previous_factor,
            reserved: [0; 64],
        }
    }
}

/// An event indicating that builder fees have been claimed.
///
/// Emitted only when the claim actually transfers a non-zero amount; the
/// no-op path (an empty claim vault) emits nothing.
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
    /// The builder's User Account.
    pub builder: Pubkey,
    /// The token mint the claim is denominated in.
    pub token: Pubkey,
    /// The amount transferred, i.e. the claim vault's full prior balance.
    pub amount: u64,
    /// The destination token account the claim was sent to.
    pub destination: Pubkey,
}

impl BuilderFeeClaimed {
    pub(crate) fn new(
        store: &Pubkey,
        builder: &Pubkey,
        token: &Pubkey,
        amount: u64,
        destination: &Pubkey,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            builder: *builder,
            token: *token,
            amount,
            destination: *destination,
        })
    }
}

impl InitSpace for BuilderFeeClaimed {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for BuilderFeeClaimed {}
