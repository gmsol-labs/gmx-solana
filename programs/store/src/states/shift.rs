use anchor_lang::prelude::*;

use crate::events::ShiftRemoved;

use super::{
    common::{
        action::{Action, ActionHeader, Closable},
        token::TokenAndAccount,
    },
    Seed,
};

/// Shift.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Shift {
    /// Action header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: ShiftTokenAccounts,
    /// Shift params.
    pub(crate) params: ShiftActionParams,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl Seed for Shift {
    const SEED: &'static [u8] = b"shift";
}

impl Action for Shift {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

impl Closable for Shift {
    type ClosedEvent = ShiftRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        ShiftRemoved::new(
            self.header.id,
            self.header.store,
            *address,
            self.tokens().from_market_token(),
            self.header.owner,
            self.header.action_state()?,
            reason,
        )
    }
}

impl gmsol_utils::InitSpace for Shift {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Shift {
    /// Get token infos.
    pub fn tokens(&self) -> &ShiftTokenAccounts {
        &self.tokens
    }
}

#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShiftTokenAccounts {
    pub(crate) from_market_token: TokenAndAccount,
    pub(crate) to_market_token: TokenAndAccount,
    pub(crate) long_token: Pubkey,
    pub(crate) short_token: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl ShiftTokenAccounts {
    /// Get from market token.
    pub fn from_market_token(&self) -> Pubkey {
        self.from_market_token.token().expect("must exist")
    }

    /// Get from market token account.
    pub fn from_market_token_account(&self) -> Pubkey {
        self.from_market_token.account().expect("must exist")
    }

    /// Get to market token.
    pub fn to_market_token(&self) -> Pubkey {
        self.to_market_token.token().expect("must exist")
    }

    /// Get to market token account.
    pub fn to_market_token_account(&self) -> Pubkey {
        self.to_market_token.account().expect("must exist")
    }

    /// Get long token.
    pub fn long_token(&self) -> &Pubkey {
        &self.long_token
    }

    /// Get short token.
    pub fn short_token(&self) -> &Pubkey {
        &self.short_token
    }
}

#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShiftActionParams {
    pub(crate) from_market_token_amount: u64,
    pub(crate) min_to_market_token_amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl ShiftActionParams {
    /// Get from market token amount.
    pub fn from_market_token_amount(&self) -> u64 {
        self.from_market_token_amount
    }

    /// Get min to market token amount.
    pub fn min_to_market_token_amount(&self) -> u64 {
        self.min_to_market_token_amount
    }
}
