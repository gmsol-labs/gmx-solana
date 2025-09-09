use std::{
    collections::HashMap,
    fmt,
    iter::{Peekable, Zip},
    slice::Iter,
    sync::Arc,
};

use bytes::Bytes;
use gmsol_programs::{
    anchor_lang::{self, AccountDeserialize},
    gmsol_store::accounts::TokenMapHeader,
};
use gmsol_utils::{
    dynamic_access,
    token_config::{TokenConfig, TokenMapAccess},
};
use solana_sdk::pubkey::Pubkey;

use gmsol_utils::{oracle::PriceProviderKind, token_config::TokensWithFeed};
use solana_sdk::instruction::AccountMeta;

use crate::utils::zero_copy::{check_discriminator, try_deserialize_unchecked};

/// Token Map.
#[derive(Debug, Clone)]
pub struct TokenMap {
    header: Arc<TokenMapHeader>,
    configs: Bytes,
}

impl TokenMapAccess for TokenMap {
    fn get(&self, token: &Pubkey) -> Option<&TokenConfig> {
        let index = usize::from(*self.header.tokens.get(token)?);
        dynamic_access::get(&self.configs, index)
    }
}

impl TokenMap {
    /// Get the header.
    pub fn header(&self) -> &TokenMapHeader {
        &self.header
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.header.tokens.is_empty()
    }

    /// Get the number of tokens in the map.
    pub fn len(&self) -> usize {
        self.header.tokens.len()
    }

    /// Get all tokens.
    pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.header
            .tokens
            .entries()
            .map(|(k, _)| Pubkey::new_from_array(*k))
    }

    /// Create an iterator over the entires of the map.
    pub fn iter(&self) -> impl Iterator<Item = (Pubkey, &TokenConfig)> + '_ {
        self.tokens()
            .filter_map(|token| self.get(&token).map(|config| (token, config)))
    }
}

impl AccountDeserialize for TokenMap {
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        check_discriminator::<TokenMapHeader>(buf)?;
        Self::try_deserialize_unchecked(buf)
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let header = Arc::new(try_deserialize_unchecked::<TokenMapHeader>(buf)?);
        let (_disc, data) = buf.split_at(8);
        let (_header, configs) = data.split_at(std::mem::size_of::<TokenMapHeader>());
        Ok(Self {
            header,
            configs: Bytes::copy_from_slice(configs),
        })
    }
}

type Parser = Arc<dyn Fn(Pubkey) -> crate::Result<AccountMeta>>;

/// A mapping from feed id to the corresponding feed address.
pub type FeedAddressMap = std::collections::HashMap<Pubkey, Pubkey>;

/// Feeds parser.
#[derive(Default, Clone)]
pub struct FeedsParser {
    parsers: HashMap<PriceProviderKind, Parser>,
}

impl fmt::Debug for FeedsParser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedsParser").finish_non_exhaustive()
    }
}

impl FeedsParser {
    /// Parse a [`TokensWithFeed`]
    pub fn parse<'a>(
        &'a self,
        tokens_with_feed: &'a TokensWithFeed,
    ) -> impl Iterator<Item = crate::Result<AccountMeta>> + 'a {
        Feeds::new(tokens_with_feed).map(|res| {
            res.and_then(|FeedConfig { provider, feed, .. }| self.dispatch(&provider, &feed))
        })
    }

    /// Parse and sort by tokens.
    pub fn parse_and_sort_by_tokens(
        &self,
        tokens_with_feed: &TokensWithFeed,
    ) -> crate::Result<Vec<AccountMeta>> {
        let accounts = self
            .parse(tokens_with_feed)
            .collect::<crate::Result<Vec<_>>>()?;

        let mut combined = tokens_with_feed
            .tokens
            .iter()
            .zip(accounts)
            .collect::<Vec<_>>();

        combined.sort_by_key(|(key, _)| *key);

        Ok(combined.into_iter().map(|(_, account)| account).collect())
    }

    fn dispatch(&self, provider: &PriceProviderKind, feed: &Pubkey) -> crate::Result<AccountMeta> {
        let Some(parser) = self.parsers.get(provider) else {
            return Ok(AccountMeta {
                pubkey: *feed,
                is_signer: false,
                is_writable: false,
            });
        };
        (parser)(*feed)
    }

    /// Insert a pull oracle feed parser.
    pub fn insert_pull_oracle_feed_parser(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> &mut Self {
        self.parsers.insert(
            provider,
            Arc::new(move |feed_id| {
                let price_update = map.get(&feed_id).ok_or_else(|| {
                    crate::Error::custom(format!("feed account for {feed_id} not provided"))
                })?;

                Ok(AccountMeta {
                    pubkey: *price_update,
                    is_signer: false,
                    is_writable: false,
                })
            }),
        );
        self
    }
}

/// Feed account metas.
pub struct Feeds<'a> {
    provider_with_lengths: Peekable<Zip<Iter<'a, u8>, Iter<'a, u16>>>,
    tokens: Iter<'a, Pubkey>,
    feeds: Iter<'a, Pubkey>,
    current: usize,
    failed: bool,
}

impl<'a> Feeds<'a> {
    /// Create from [`TokensWithFeed`].
    pub fn new(token_with_feeds: &'a TokensWithFeed) -> Self {
        let providers = token_with_feeds.providers.iter();
        let nums = token_with_feeds.nums.iter();
        let provider_with_lengths = providers.zip(nums).peekable();
        let tokens = token_with_feeds.tokens.iter();
        let feeds = token_with_feeds.feeds.iter();
        Self {
            provider_with_lengths,
            tokens,
            feeds,
            current: 0,
            failed: false,
        }
    }
}

impl Iterator for Feeds<'_> {
    type Item = crate::Result<FeedConfig>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        loop {
            let (provider, length) = self.provider_with_lengths.peek()?;
            if self.current == (**length as usize) {
                self.provider_with_lengths.next();
                self.current = 0;
                continue;
            }
            let Ok(provider) = PriceProviderKind::try_from(**provider) else {
                self.failed = true;
                return Some(Err(crate::Error::custom("invalid provider index")));
            };
            let Some(feed) = self.feeds.next() else {
                return Some(Err(crate::Error::custom("not enough feeds")));
            };
            let Some(token) = self.tokens.next() else {
                return Some(Err(crate::Error::custom("not enough tokens")));
            };
            self.current += 1;
            return Some(Ok(FeedConfig {
                token: *token,
                provider,
                feed: *feed,
            }));
        }
    }
}

/// A feed config.
#[derive(Debug, Clone)]
pub struct FeedConfig {
    /// Token.
    pub token: Pubkey,
    /// Provider Kind.
    pub provider: PriceProviderKind,
    /// Feed.
    pub feed: Pubkey,
}
