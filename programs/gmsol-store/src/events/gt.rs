use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use crate::states::{gt::GtState, user};

use super::Event;

/// GT updated event.
#[event]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(InitSpace)]
pub struct GtUpdated {
    /// Update kind.
    pub kind: GtUpdateKind,
    /// Receiver.
    pub receiver: Option<Pubkey>,
    /// Receiver Delta.
    pub receiver_delta: u64,
    /// Receiver balance.
    pub receiver_balance: Option<u64>,
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

/// GT Update Kind.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub enum GtUpdateKind {
    /// Reward.
    Reward,
    /// Mint.
    Mint,
    /// Burn.
    Burn,
}

impl gmsol_utils::InitSpace for GtUpdated {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for GtUpdated {}

impl GtUpdated {
    fn new(
        kind: GtUpdateKind,
        minting_cost: Option<u128>,
        delta: u64,
        state: &GtState,
        receiver: Option<&user::UserHeader>,
    ) -> Self {
        Self {
            kind,
            receiver: receiver.map(|header| header.owner),
            receiver_delta: delta,
            receiver_balance: receiver.map(|header| header.gt().amount()),
            minting_cost: minting_cost.unwrap_or(state.minting_cost()),
            total_minted: state.total_minted(),
            grow_steps: state.grow_steps(),
            supply: state.supply(),
            vault: state.gt_vault(),
            reserved: [0; 64],
        }
    }

    /// Create a new rewarded event.
    pub fn rewarded(amount: u64, state: &GtState, receiver: Option<&user::UserHeader>) -> Self {
        Self::new(GtUpdateKind::Reward, None, amount, state, receiver)
    }

    /// Create a new minted event.
    pub fn minted(
        minting_cost: u128,
        amount: u64,
        state: &GtState,
        receiver: Option<&user::UserHeader>,
    ) -> Self {
        Self::new(
            GtUpdateKind::Mint,
            Some(minting_cost),
            amount,
            state,
            receiver,
        )
    }

    /// Create a new burned event.
    pub fn burned(amount: u64, state: &GtState, receiver: Option<&user::UserHeader>) -> Self {
        Self::new(GtUpdateKind::Burn, None, amount, state, receiver)
    }
}

/// Event indicating that a GT buyback has occurred.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct GtBuyback {
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// GT exchange vault.
    pub gt_exchange_vault: Pubkey,
    /// Authority.
    pub authority: Pubkey,
    /// Total buyback amount.
    pub buyback_amount: u64,
    /// Buyback value.
    pub buyback_value: Option<u128>,
    /// Buyback price.
    pub buyback_price: Option<u128>,
}

impl GtBuyback {
    pub(crate) fn new(
        store: &Pubkey,
        gt_exchange_vault: &Pubkey,
        authority: &Pubkey,
        buyback_amount: u64,
        buyback_value: Option<u128>,
        buyback_price: Option<u128>,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            gt_exchange_vault: *gt_exchange_vault,
            authority: *authority,
            buyback_amount,
            buyback_value,
            buyback_price,
        })
    }
}

impl gmsol_utils::InitSpace for GtBuyback {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for GtBuyback {}
