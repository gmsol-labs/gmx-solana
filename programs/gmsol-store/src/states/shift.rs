use anchor_lang::prelude::*;

use crate::{events::ShiftRemoved, states::Deposit, CoreError};

use super::{
    common::{
        action::{Action, ActionHeader, Closable},
        token::TokenAndAccount,
    },
    Market, Seed,
};

/// Shift.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Shift {
    /// Action header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: TokenAccounts,
    /// Shift params.
    pub(crate) params: ShiftParams,
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
    pub fn tokens(&self) -> &TokenAccounts {
        &self.tokens
    }

    /// Validate the shift params for execution.
    pub(crate) fn validate_for_execution(
        &self,
        to_market_token: &AccountInfo,
        to_market: &Market,
    ) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        require_eq!(
            *to_market_token.key,
            self.tokens().to_market_token(),
            CoreError::MarketTokenMintMismatched
        );

        let supply = amount(to_market_token)?;

        if supply == 0 {
            Deposit::validate_first_deposit(
                &self.header.owner,
                self.params.min_to_market_token_amount,
                to_market,
            )?;
        }

        Ok(())
    }
}

#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenAccounts {
    pub(crate) from_market_token: TokenAndAccount,
    pub(crate) to_market_token: TokenAndAccount,
    pub(crate) long_token: Pubkey,
    pub(crate) short_token: Pubkey,
}

impl TokenAccounts {
    /// Get from market token.
    pub fn from_market_token(&self) -> Pubkey {
        self.from_market_token.token().expect("must exist")
    }

    /// Get from market token account.
    pub fn from_market_token_account(&self) -> Pubkey {
        self.from_market_token.account().expect("msut exist")
    }

    /// Get to market token.
    pub fn to_market_token(&self) -> Pubkey {
        self.to_market_token.token().expect("must exist")
    }

    /// Get to market token account.
    pub fn to_market_token_account(&self) -> Pubkey {
        self.to_market_token.account().expect("msut exist")
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

#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct ShiftParams {
    pub(crate) from_market_token_amount: u64,
    pub(crate) min_to_market_token_amount: u64,
}

impl ShiftParams {
    /// Get from market token amount.
    pub fn from_market_token_amount(&self) -> u64 {
        self.from_market_token_amount
    }

    /// Get min to market token amount.
    pub fn min_to_market_token_amount(&self) -> u64 {
        self.min_to_market_token_amount
    }
}
