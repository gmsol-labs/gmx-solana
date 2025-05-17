use anchor_lang::prelude::*;
use anchor_spl::{token::TokenAccount, token_interface};

use crate::utils::pubkey::optional_address;

pub use gmsol_utils::token_config::TokensCollector;

/// Token Account.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokenAndAccount {
    /// Token.
    token: Pubkey,
    /// Account.
    account: Pubkey,
}

impl TokenAndAccount {
    /// Initialize with token account.
    pub fn init(&mut self, account: &Account<TokenAccount>) {
        self.token = account.mint;
        self.account = account.key();
    }

    /// Initialize with token account interface.
    pub fn init_with_interface(
        &mut self,
        account: &InterfaceAccount<token_interface::TokenAccount>,
    ) {
        self.token = account.mint;
        self.account = account.key();
    }

    /// Get token.
    pub fn token(&self) -> Option<Pubkey> {
        optional_address(&self.token).copied()
    }

    /// Get account.
    pub fn account(&self) -> Option<Pubkey> {
        optional_address(&self.account).copied()
    }

    /// Get token and account.
    pub fn token_and_account(&self) -> Option<(Pubkey, Pubkey)> {
        let token = self.token()?;
        let account = self.account()?;
        Some((token, account))
    }
}
