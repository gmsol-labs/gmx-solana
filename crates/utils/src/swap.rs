use std::collections::HashSet;

use anchor_lang::prelude::*;

use crate::token_config::{
    TokenConfigError, TokenConfigResult, TokenMapAccess, TokenRecord, TokensWithFeed,
};

const MAX_STEPS: usize = 10;
const MAX_TOKENS: usize = 2 * MAX_STEPS + 2 + 3;

/// Swap Parameter error.
#[derive(Debug, thiserror::Error)]
pub enum SwapActionParamsError {
    /// Invalid swap path.
    #[error("invalid swap path: {0}")]
    InvalidSwapPath(&'static str),
}

type SwapActionParamsResult<T> = std::result::Result<T, SwapActionParamsError>;

/// Swap params.
#[zero_copy]
#[derive(Default)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SwapActionParams {
    /// The length of primary swap path.
    pub primary_length: u8,
    /// The length of secondary swap path.
    pub secondary_length: u8,
    /// The number of tokens.
    pub num_tokens: u8,
    /// Padding.
    pub padding_0: [u8; 1],
    pub current_market_token: Pubkey,
    /// Swap paths.
    pub paths: [Pubkey; MAX_STEPS],
    /// Tokens.
    pub tokens: [Pubkey; MAX_TOKENS],
}

impl SwapActionParams {
    /// Max total length of swap paths.
    pub const MAX_TOTAL_LENGTH: usize = MAX_STEPS;

    /// Max total number of tokens of swap path.
    pub const MAX_TOKENS: usize = MAX_TOKENS;

    /// Get the length of primary swap path.
    pub fn primary_length(&self) -> usize {
        usize::from(self.primary_length)
    }

    /// Get the length of secondary swap path.
    pub fn secondary_length(&self) -> usize {
        usize::from(self.secondary_length)
    }

    /// Get the number of tokens.
    pub fn num_tokens(&self) -> usize {
        usize::from(self.num_tokens)
    }

    /// Get primary swap path.
    pub fn primary_swap_path(&self) -> &[Pubkey] {
        let end = self.primary_length();
        &self.paths[0..end]
    }

    /// Get secondary swap path.
    pub fn secondary_swap_path(&self) -> &[Pubkey] {
        let start = self.primary_length();
        let end = start.saturating_add(self.secondary_length());
        &self.paths[start..end]
    }

    /// Get validated primary swap path.
    pub fn validated_primary_swap_path(&self) -> SwapActionParamsResult<&[Pubkey]> {
        let mut seen: HashSet<&Pubkey> = HashSet::default();
        if !self
            .primary_swap_path()
            .iter()
            .all(move |token| seen.insert(token))
        {
            return Err(SwapActionParamsError::InvalidSwapPath("primary"));
        }

        Ok(self.primary_swap_path())
    }

    /// Get validated secondary swap path.
    pub fn validated_secondary_swap_path(&self) -> SwapActionParamsResult<&[Pubkey]> {
        let mut seen: HashSet<&Pubkey> = HashSet::default();
        if !self
            .secondary_swap_path()
            .iter()
            .all(move |token| seen.insert(token))
        {
            return Err(SwapActionParamsError::InvalidSwapPath("secondary"));
        }

        Ok(self.secondary_swap_path())
    }

    /// Get all tokens for the action.
    pub fn tokens(&self) -> &[Pubkey] {
        let end = self.num_tokens();
        &self.tokens[0..end]
    }

    /// Convert to token records.
    pub fn to_token_records<'a>(
        &'a self,
        map: &'a impl TokenMapAccess,
    ) -> impl Iterator<Item = TokenConfigResult<TokenRecord>> + 'a {
        self.tokens().iter().map(|token| {
            let config = map.get(token).ok_or(TokenConfigError::NotFound)?;
            TokenRecord::from_config(*token, config)
        })
    }

    /// Convert to tokens with feed.
    pub fn to_feeds(&self, map: &impl TokenMapAccess) -> TokenConfigResult<TokensWithFeed> {
        let records = self
            .to_token_records(map)
            .collect::<TokenConfigResult<Vec<_>>>()?;
        TokensWithFeed::try_from_records(records)
    }

    /// Iterate over both swap paths, primary path first then secondary path.
    pub fn iter(&self) -> impl Iterator<Item = &Pubkey> {
        self.primary_swap_path()
            .iter()
            .chain(self.secondary_swap_path().iter())
    }

    /// Get unique market tokens excluding current market token.
    pub fn unique_market_tokens_excluding_current<'a>(
        &'a self,
        current_market_token: &'a Pubkey,
    ) -> impl Iterator<Item = &'a Pubkey> + 'a {
        let mut seen = HashSet::from([current_market_token]);
        self.iter().filter(move |token| seen.insert(token))
    }

    /// Get the first market token in the swap path.
    pub fn first_market_token(&self, is_primary: bool) -> Option<&Pubkey> {
        if is_primary {
            self.primary_swap_path().first()
        } else {
            self.secondary_swap_path().first()
        }
    }

    /// Get the last market token in the swap path.
    pub fn last_market_token(&self, is_primary: bool) -> Option<&Pubkey> {
        if is_primary {
            self.primary_swap_path().last()
        } else {
            self.secondary_swap_path().last()
        }
    }
}

/// Has swap parameters.
pub trait HasSwapParams {
    /// Get the swap params.
    fn swap(&self) -> &SwapActionParams;
}

impl HasSwapParams for SwapActionParams {
    fn swap(&self) -> &SwapActionParams {
        self
    }
}
