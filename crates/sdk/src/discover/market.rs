use std::{collections::BTreeMap, sync::Arc, task::Poll, time::Duration};

use futures_util::Stream;
use gmsol_programs::gmsol_store::accounts::Market;
use gmsol_solana_utils::{
    cluster::Cluster,
    signer::{shared_signer, SignerRef},
    utils::WithSlot,
};
use gmsol_utils::market::{MarketFlag, MarketMeta};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair};
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;
use tower::discover::Change;
use tracing::Instrument;

use crate::{
    client::{accounts::ProgramAccountsConfig, ClientOptions},
    pda::find_default_store_address,
    Client,
};

pub(crate) type MarketSvc = MarketMeta;
type SharedMarket = Arc<Market>;
type Cache = BTreeMap<Pubkey, SharedMarket>;

pin_project_lite::pin_project! {
    /// Market Discovery.
    pub struct MarketDiscovery {
        #[pin]
        stream: WatchStream<Cache>,
        sender: watch::Sender<Cache>,
        cache: Cache,
        changes: std::vec::IntoIter<Change<Pubkey, MarketSvc>>,
    }
}

impl Clone for MarketDiscovery {
    fn clone(&self) -> Self {
        let mut receiver = self.sender.subscribe();
        let changes = receiver
            .borrow_and_update()
            .iter()
            .map(|(pubkey, m)| Change::Insert(*pubkey, m.meta.into()))
            .collect::<Vec<_>>();
        Self {
            stream: WatchStream::new(receiver),
            sender: self.sender.clone(),
            cache: Default::default(),
            changes: changes.into_iter(),
        }
    }
}

impl MarketDiscovery {
    /// Create a new market discovery service for the default store.
    pub fn new(cluster: Cluster) -> crate::Result<Self> {
        Self::new_with_store(cluster, find_default_store_address().0)
    }

    /// Create a new market discovery service for the given store with default [`Client`] and options.
    pub fn new_with_store(cluster: Cluster, store: Pubkey) -> crate::Result<Self> {
        let client = Client::new_with_options(
            cluster,
            shared_signer(Keypair::new()),
            ClientOptions::default(),
        )?;
        Self::new_with_options(client, store, Duration::from_secs(30))
    }

    /// Create a new market discovery service with options.
    pub fn new_with_options(
        client: Client<SignerRef>,
        store: Pubkey,
        interval: Duration,
    ) -> crate::Result<Self> {
        let (sender, receiver) = watch::channel(Cache::default());
        let watcher = Watcher {
            store,
            interval,
            client,
            sender: sender.clone(),
        };

        let worker = watcher
            .run()
            .instrument(tracing::debug_span!("market_watcher", %store));

        tokio::spawn(
            async move {
                match worker.await {
                    Ok(()) => {
                        tracing::warn!("stopped");
                    }
                    Err(err) => {
                        tracing::warn!(%err, "stopped with error");
                    }
                }
            }
            .in_current_span(),
        );

        Ok(Self {
            sender,
            stream: WatchStream::new(receiver),
            cache: Cache::default(),
            changes: Default::default(),
        })
    }
}

impl Stream for MarketDiscovery {
    type Item = crate::Result<Change<Pubkey, MarketSvc>>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        loop {
            if let Some(change) = this.changes.next() {
                return Poll::Ready(Some(Ok(change)));
            }
            let Some(markets) = futures_util::ready!(this.stream.as_mut().poll_next(cx)) else {
                return Poll::Ready(Some(Err(crate::Error::custom("disconnected"))));
            };
            let mut changes = Vec::default();
            for (pubkey, market) in markets.iter() {
                if !this.cache.contains_key(pubkey) {
                    changes.push(Change::Insert(*pubkey, market.meta.into()));
                }
            }
            for pubkey in this.cache.keys() {
                if !markets.contains_key(pubkey) {
                    changes.push(Change::Remove(*pubkey));
                }
            }
            tracing::debug!(len = %changes.len(), "updated");
            *this.cache = markets;
            *this.changes = changes.into_iter();
        }
    }
}

struct Watcher {
    store: Pubkey,
    interval: Duration,
    client: Client<SignerRef>,
    sender: watch::Sender<Cache>,
}

impl Watcher {
    async fn run(self) -> crate::Result<()> {
        let mut interval = tokio::time::interval(self.interval);
        loop {
            interval.tick().await;
            let Ok(markets) = self.fetch_once().await else {
                continue;
            };
            let slot = markets.slot();
            let cache = markets.into_value();
            tracing::debug!(%slot, len=%cache.len(), "fetched new market list");
            self.sender.send(cache).map_err(crate::Error::custom)?;
        }
    }

    async fn fetch_once(&self) -> crate::Result<WithSlot<BTreeMap<Pubkey, Arc<Market>>>> {
        let mut markets = self
            .client
            .markets_with_config(
                &self.store,
                ProgramAccountsConfig {
                    commitment: Some(CommitmentConfig::finalized()),
                    ..Default::default()
                },
            )
            .await?;
        markets
            .value_mut()
            .retain(|_pubkey, m| m.flags.get_flag(MarketFlag::Enabled));
        Ok(markets)
    }
}

#[cfg(test)]
mod tests {
    use futures_util::{future::poll_fn, pin_mut};
    use tokio::time::timeout;
    use tower::discover::Discover;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn test_market_discover() -> eyre::Result<()> {
        let _guard = crate::test::setup_fmt_tracing("info,gmsol::discover=debug");
        let markets = MarketDiscovery::new(crate::test::default_cluster())?;

        pin_mut!(markets);
        if let Some(Ok(change)) =
            timeout(TIMEOUT, poll_fn(|cx| markets.as_mut().poll_discover(cx))).await?
        {
            tracing::info!("{change:?}");
        }
        let cloned_markets = markets.clone();
        pin_mut!(cloned_markets);
        if let Some(Ok(change)) = timeout(
            TIMEOUT,
            poll_fn(|cx| cloned_markets.as_mut().poll_discover(cx)),
        )
        .await?
        {
            tracing::info!("{change:?}");
        }
        Ok(())
    }
}
