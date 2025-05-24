use anchor_lang::prelude::*;

use crate::{events::WithdrawalRemoved, CoreError};

use super::{
    common::{
        action::{Action, ActionHeader, Closable},
        swap::SwapActionParams,
        token::TokenAndAccount,
    },
    Seed,
};

/// Withdrawal.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Withdrawal {
    /// Action header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: WithdrawalTokenAccounts,
    /// Withdrawal params.
    pub(crate) params: WithdrawalActionParams,
    /// Swap params.
    pub(crate) swap: SwapActionParams,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 4],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl Withdrawal {
    /// Get tokens and accounts.
    pub fn tokens(&self) -> &WithdrawalTokenAccounts {
        &self.tokens
    }

    /// Get the swap params.
    pub fn swap(&self) -> &SwapActionParams {
        &self.swap
    }
}

impl Seed for Withdrawal {
    /// Seed.
    const SEED: &'static [u8] = b"withdrawal";
}

impl gmsol_utils::InitSpace for Withdrawal {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Action for Withdrawal {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

impl Closable for Withdrawal {
    type ClosedEvent = WithdrawalRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        WithdrawalRemoved::new(
            self.header.id,
            self.header.store,
            *address,
            self.tokens.market_token(),
            self.header.owner,
            self.header.action_state()?,
            reason,
        )
    }
}

/// Token Accounts.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WithdrawalTokenAccounts {
    /// Final long token accounts.
    pub(crate) final_long_token: TokenAndAccount,
    /// Final short token accounts.
    pub(crate) final_short_token: TokenAndAccount,
    /// Market token account.
    pub(crate) market_token: TokenAndAccount,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl WithdrawalTokenAccounts {
    /// Get market token.
    pub fn market_token(&self) -> Pubkey {
        self.market_token.token().expect("must exist")
    }

    /// Get market token account.
    pub fn market_token_account(&self) -> Pubkey {
        self.market_token.account().expect("must exist")
    }

    /// Get final_long token.
    pub fn final_long_token(&self) -> Pubkey {
        self.final_long_token.token().expect("must exist")
    }

    /// Get final_long token account.
    pub fn final_long_token_account(&self) -> Pubkey {
        self.final_long_token.account().expect("must exist")
    }

    /// Get final_short token.
    pub fn final_short_token(&self) -> Pubkey {
        self.final_short_token.token().expect("must exist")
    }

    /// Get final_short token account.
    pub fn final_short_token_account(&self) -> Pubkey {
        self.final_short_token.account().expect("must exist")
    }
}

/// Withdrawal params.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WithdrawalActionParams {
    /// Market token amount to burn.
    pub market_token_amount: u64,
    /// The minimum acceptable amount of final long tokens to receive.
    pub min_long_token_amount: u64,
    /// The minimum acceptable amount of final short tokens to receive.
    pub min_short_token_amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl Default for WithdrawalActionParams {
    fn default() -> Self {
        Self {
            reserved: [0; 64],
            market_token_amount: 0,
            min_long_token_amount: 0,
            min_short_token_amount: 0,
        }
    }
}

impl WithdrawalActionParams {
    pub(crate) fn validate_output_amounts(
        &self,
        long_amount: u64,
        short_amount: u64,
    ) -> Result<()> {
        require_gte!(
            long_amount,
            self.min_long_token_amount,
            CoreError::InsufficientOutputAmount
        );
        require_gte!(
            short_amount,
            self.min_short_token_amount,
            CoreError::InsufficientOutputAmount
        );
        Ok(())
    }
}
