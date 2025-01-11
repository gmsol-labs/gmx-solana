use anchor_lang::prelude::*;
use anchor_spl::{token::TokenAccount, token_interface};

use crate::{states::TokenMapAccess, utils::pubkey::optional_address, CoreError};

use super::{swap::HasSwapParams, TokenRecord, TokensWithFeed};

/// Token Account.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
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

/// Tokens Collector.
pub struct TokensCollector {
    tokens: Vec<Pubkey>,
}

impl TokensCollector {
    /// Create a new [`TokensCollector`].
    pub fn new(action: Option<&impl HasSwapParams>, extra_capacity: usize) -> Self {
        let mut tokens;
        match action {
            Some(action) => {
                let swap = action.swap();
                tokens = Vec::with_capacity(swap.num_tokens() + extra_capacity);
                // The tokens in the swap params must be sorted.
                tokens.extend_from_slice(swap.tokens());
            }
            None => tokens = Vec::with_capacity(extra_capacity),
        }

        Self { tokens }
    }

    /// Insert a new token.
    pub fn insert_token(&mut self, token: &Pubkey) -> bool {
        match self.tokens.binary_search(token) {
            Ok(_) => false,
            Err(idx) => {
                self.tokens.insert(idx, *token);
                true
            }
        }
    }

    /// Convert to a vec.
    pub fn into_vec(mut self, token_map: &impl TokenMapAccess) -> Result<Vec<Pubkey>> {
        token_map.sort_tokens_by_provider(&mut self.tokens)?;
        Ok(self.tokens)
    }

    /// Convert to [`TokensWithFeed`].
    pub fn to_feeds(&self, token_map: &impl TokenMapAccess) -> Result<TokensWithFeed> {
        let records = self
            .tokens
            .iter()
            .map(|token| {
                let config = token_map
                    .get(token)
                    .ok_or_else(|| error!(CoreError::UnknownOrDisabledToken))?;
                TokenRecord::from_config(*token, config)
            })
            .collect::<Result<Vec<_>>>()?;
        TokensWithFeed::try_from_records(records)
    }
}
