use std::{
    collections::HashMap,
    iter::{Peekable, Zip},
    slice::Iter,
};

use gmsol_utils::{oracle::PriceProviderKind, token_config::TokensWithFeed};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

type Parser = Box<dyn Fn(Pubkey) -> crate::Result<AccountMeta>>;

/// A mapping from feed id to the corresponding feed address.
pub type FeedAddressMap = std::collections::HashMap<Pubkey, Pubkey>;

/// Feeds parser.
#[derive(Default)]
pub struct FeedsParser {
    parsers: HashMap<PriceProviderKind, Parser>,
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
            Box::new(move |feed_id| {
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
