use std::collections::BTreeSet;

use anchor_lang::prelude::*;

use crate::{
    states::{TokenConfig, TokenMapAccess},
    utils::chunk_by::chunk_by,
    CoreError,
};

use super::PriceProviderKind;

/// Tokens with feed.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokensWithFeed {
    /// Tokens that require prices,
    /// which must be of the same length with `feeds`.
    pub tokens: Vec<Pubkey>,
    /// Token feeds for the tokens,
    /// which must be of the same length with `tokens`.
    pub feeds: Vec<Pubkey>,
    /// Providers set,
    /// which must be of the same length with `nums`.
    pub providers: Vec<u8>,
    /// The numbers of tokens of each provider.
    pub nums: Vec<u16>,
}

/// A record of token config.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokenRecord {
    token: Pubkey,
    feed: Pubkey,
    provider: u8,
}

impl TokenRecord {
    /// Create a new [`TokenRecord`]
    pub fn new(token: Pubkey, feed: Pubkey, provider: PriceProviderKind) -> Self {
        Self {
            token,
            feed,
            provider: provider as u8,
        }
    }

    /// Create a new [`TokenRecord`] from token config,
    /// using the expected provider and feed.
    pub fn from_config(token: Pubkey, config: &TokenConfig) -> Result<Self> {
        Ok(Self::new(
            token,
            config.get_expected_feed()?,
            config.expected_provider()?,
        ))
    }
}

impl TokensWithFeed {
    /// Create from token records.
    /// # Panic
    /// Panics if the number of tokens of the same provider exceeds `u16`.
    pub fn try_from_records(mut records: Vec<TokenRecord>) -> Result<Self> {
        records.sort_by_cached_key(|r| r.provider);
        let mut chunks = chunk_by(&records, |a, b| a.provider == b.provider);
        let capacity = chunks.size_hint().0;
        let mut providers = Vec::with_capacity(capacity);
        let mut nums = Vec::with_capacity(capacity);
        chunks.try_for_each(|chunk| {
            providers.push(chunk[0].provider);
            nums.push(u16::try_from(chunk.len()).map_err(|_| CoreError::ExceedMaxLengthLimit)?);
            Result::Ok(())
        })?;
        Ok(Self {
            tokens: records.iter().map(|r| r.token).collect(),
            feeds: records.iter().map(|r| r.feed).collect(),
            providers,
            nums,
        })
    }
}

/// Collect token records for the give tokens.
pub fn token_records<A: TokenMapAccess>(
    token_map: &A,
    tokens: &BTreeSet<Pubkey>,
) -> Result<Vec<TokenRecord>> {
    tokens
        .iter()
        .map(|token| {
            let config = token_map
                .get(token)
                .ok_or_else(|| error!(CoreError::NotFound))?;
            TokenRecord::from_config(*token, config)
        })
        .collect::<Result<Vec<_>>>()
}
