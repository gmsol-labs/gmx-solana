use std::{
    collections::{HashMap, HashSet},
    task::Poll,
};

use futures_util::Stream;
use gmsol_utils::market::MarketMeta;
use solana_sdk::pubkey::Pubkey;
use tower::discover::{Change, Discover};

use super::market::MarketSvc;

pub(crate) type TokenSvc = ();
type Markets = HashSet<Pubkey>;
type MarketMetas = HashMap<Pubkey, MarketMeta>;
type Cache = HashMap<Pubkey, Markets>;

pin_project_lite::pin_project! {
    /// Token Discovery.
    pub struct TokenDiscovery<D> {
        #[pin]
        market: D,
        cache: Cache,
        metas: MarketMetas,
        changes: std::vec::IntoIter<Change<Pubkey, TokenSvc>>,
    }
}

impl<D> TokenDiscovery<D>
where
    D: Discover<Key = Pubkey, Service = MarketSvc, Error = crate::Error>,
{
    /// Create a new token discovery service with the given market discovery service.
    pub fn new(market_discovery: D) -> Self {
        Self {
            market: market_discovery,
            cache: Cache::default(),
            metas: Default::default(),
            changes: Default::default(),
        }
    }
}

impl<D> Stream for TokenDiscovery<D>
where
    D: Discover<Key = Pubkey, Service = MarketSvc, Error = crate::Error>,
{
    type Item = crate::Result<Change<Pubkey, TokenSvc>>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        use std::collections::hash_map::Entry;

        let mut this = self.project();
        loop {
            if let Some(change) = this.changes.next() {
                return Poll::Ready(Some(Ok(change)));
            }
            let Some(res) = futures_util::ready!(this.market.as_mut().poll_discover(cx)) else {
                return Poll::Ready(None);
            };
            let mut changes = Vec::default();
            match res {
                Ok(change) => match change {
                    Change::Insert(market, svc) => {
                        for token in tokens(&svc) {
                            match this.cache.entry(token) {
                                Entry::Occupied(mut entry) => {
                                    entry.get_mut().insert(market);
                                }
                                Entry::Vacant(entry) => {
                                    entry.insert(Markets::from([market]));
                                    changes.push(Change::Insert(token, ()));
                                }
                            }
                        }
                        this.metas.insert(market, svc);
                    }
                    Change::Remove(market) => {
                        if let Some(meta) = this.metas.remove(&market) {
                            for token in tokens(&meta) {
                                if let Entry::Occupied(mut entry) = this.cache.entry(token) {
                                    let markets = entry.get_mut();
                                    markets.remove(&market);
                                    if markets.is_empty() {
                                        entry.remove();
                                        changes.push(Change::Remove(token));
                                    }
                                }
                            }
                        }
                    }
                },
                Err(err) => {
                    return Poll::Ready(Some(Err(err)));
                }
            }
            *this.changes = changes.into_iter();
        }
    }
}

fn tokens(svc: &MarketMeta) -> impl IntoIterator<Item = Pubkey> {
    [
        svc.index_token_mint,
        svc.long_token_mint,
        svc.short_token_mint,
    ]
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::future::poll_fn;
    use tokio::time::timeout;

    use crate::{discover::market::MarketDiscovery, test};

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn test_token_discover() -> eyre::Result<()> {
        let _guard = test::setup_fmt_tracing("info,gmsol::discover=debug");
        let markets = MarketDiscovery::new(test::default_cluster())?;
        let tokens = TokenDiscovery::new(markets);

        futures_util::pin_mut!(tokens);
        if let Some(Ok(change)) =
            timeout(TIMEOUT, poll_fn(|cx| tokens.as_mut().poll_discover(cx))).await?
        {
            tracing::info!("{change:?}");
        }
        Ok(())
    }
}
