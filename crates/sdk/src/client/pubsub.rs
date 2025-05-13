use std::{
    collections::{hash_map::Entry, HashMap},
    num::NonZeroUsize,
    ops::DerefMut,
    sync::Arc,
    time::Duration,
};

use futures_util::{Stream, StreamExt, TryStreamExt};
use gmsol_solana_utils::{
    cluster::Cluster, solana_client::rpc_response::RpcLogsResponse, utils::WithSlot,
};
use solana_client::{
    nonblocking::pubsub_client::PubsubClient as SolanaPubsubClient,
    rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter},
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use tokio::{
    sync::{broadcast, oneshot, Mutex, RwLock},
    task::{AbortHandle, JoinSet},
};
use tokio_stream::wrappers::BroadcastStream;
use tracing::Instrument;

/// A wrapper of [the solana version of pubsub client](SolanaPubsubClient)
/// with shared subscription support.
#[derive(Debug)]
pub struct PubsubClient {
    inner: RwLock<Option<Inner>>,
    cluster: Cluster,
    config: SubscriptionConfig,
}

impl PubsubClient {
    /// Create a new [`PubsubClient`] with the given config.
    pub async fn new(cluster: Cluster, config: SubscriptionConfig) -> crate::Result<Self> {
        Ok(Self {
            inner: RwLock::new(None),
            cluster,
            config,
        })
    }

    async fn prepare(&self) -> crate::Result<()> {
        if self.inner.read().await.is_some() {
            return Ok(());
        }
        self.reset().await
    }

    /// Subscribe to transaction logs.
    pub async fn logs_subscribe(
        &self,
        mention: &Pubkey,
        commitment: Option<CommitmentConfig>,
    ) -> crate::Result<impl Stream<Item = crate::Result<WithSlot<RpcLogsResponse>>>> {
        self.prepare().await?;
        let res = self
            .inner
            .read()
            .await
            .as_ref()
            .ok_or_else(|| crate::Error::unknown("the pubsub client has been closed"))?
            .logs_subscribe(mention, commitment, &self.config)
            .await;
        match res {
            Ok(stream) => Ok(stream),
            Err(crate::Error::PubsubClosed) => {
                self.reset().await?;
                Err(crate::Error::PubsubClosed)
            }
            Err(err) => Err(err),
        }
    }

    /// Reset the client.
    pub async fn reset(&self) -> crate::Result<()> {
        let client = SolanaPubsubClient::new(self.cluster.ws_url())
            .await
            .map_err(crate::Error::unknown)?;
        let mut inner = self.inner.write().await;
        if let Some(previous) = inner.take() {
            _ = previous.shutdown().await;
        }
        *inner = Some(Inner::new(client));
        Ok(())
    }

    /// Shutdown gracefully.
    pub async fn shutdown(&self) -> crate::Result<()> {
        if let Some(inner) = self.inner.write().await.take() {
            inner.shutdown().await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Inner {
    tasks: Mutex<JoinSet<()>>,
    client: Arc<SolanaPubsubClient>,
    logs: LogsSubscriptions,
}

impl Inner {
    fn new(client: SolanaPubsubClient) -> Self {
        Self {
            tasks: Default::default(),
            client: Arc::new(client),
            logs: Default::default(),
        }
    }

    async fn logs_subscribe(
        &self,
        mention: &Pubkey,
        commitment: Option<CommitmentConfig>,
        config: &SubscriptionConfig,
    ) -> crate::Result<impl Stream<Item = crate::Result<WithSlot<RpcLogsResponse>>>> {
        let config = SubscriptionConfig {
            commitment: commitment.unwrap_or(config.commitment),
            ..*config
        };
        let receiver = self
            .logs
            .subscribe(
                self.tasks.lock().await.deref_mut(),
                &self.client,
                mention,
                config,
            )
            .await?;
        Ok(BroadcastStream::new(receiver).map_err(crate::Error::unknown))
    }

    async fn shutdown(self) -> crate::Result<()> {
        self.tasks.lock().await.shutdown().await;
        Arc::into_inner(self.client)
            .ok_or_else(|| {
                crate::Error::unknown("the client should be unique here, but it is not")
            })?
            .shutdown()
            .await
            .map_err(crate::Error::unknown)?;
        Ok(())
    }
}

/// Config for subscription manager.
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    /// Commitment.
    pub commitment: CommitmentConfig,
    /// Cleanup interval.
    pub cleanup_interval: Duration,
    /// Capacity for the broadcast channel.
    pub capacity: NonZeroUsize,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            commitment: CommitmentConfig::finalized(),
            cleanup_interval: Duration::from_secs(10),
            capacity: NonZeroUsize::new(256).unwrap(),
        }
    }
}

#[derive(Debug)]
struct LogsSubscription {
    commitment: CommitmentConfig,
    sender: ClosableSender<WithSlot<RpcLogsResponse>>,
    abort: AbortHandle,
}

impl Drop for LogsSubscription {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

impl LogsSubscription {
    async fn init(
        join_set: &mut JoinSet<()>,
        sender: ClosableSender<WithSlot<RpcLogsResponse>>,
        client: &Arc<SolanaPubsubClient>,
        mention: &Pubkey,
        commitment: CommitmentConfig,
        cleanup_interval: Duration,
    ) -> crate::Result<Self> {
        let (tx, rx) = oneshot::channel::<Result<_, _>>();
        let abort = join_set.spawn({
            let client = client.clone();
            let mention = *mention;
            let sender = sender.clone();
            async move {
                let res = client
                    .logs_subscribe(
                        RpcTransactionLogsFilter::Mentions(vec![mention.to_string()]),
                        RpcTransactionLogsConfig { commitment: Some(commitment) },
                    )
                    .await
                    .inspect_err(
                        |err| tracing::error!(%err, %mention, "failed to subscribe transaction logs"),
                    );
                match res {
                    Ok((mut stream, unsubscribe)) => {
                        _ = tx.send(Ok(()));
                        let mut interval = tokio::time::interval(cleanup_interval);
                        loop {
                            tokio::select! {
                                _ = interval.tick() => {
                                    if sender.receiver_count().unwrap_or(0) == 0 {
                                        break;
                                    }
                                }
                                res = stream.next() => {
                                    match res {
                                        Some(res) => {
                                            if sender.send(WithSlot::new(res.context.slot, res.value)).unwrap_or(0) == 0 {
                                                break;
                                            }
                                        }
                                        None => break,
                                    }
                                }
                            }
                        }
                        (unsubscribe)().await;
                    },
                    Err(err) => {
                        _ = tx.send(Err(err));
                    }
                }
                tracing::info!(%mention, "logs subscription end");
            }
            .in_current_span()
        });
        rx.await
            .map_err(|_| crate::Error::unknown("worker is dead"))?
            .map_err(crate::Error::unknown)?;
        Ok(Self {
            commitment,
            abort,
            sender,
        })
    }
}

#[derive(Debug, Default)]
struct LogsSubscriptions(RwLock<HashMap<Pubkey, LogsSubscription>>);

impl LogsSubscriptions {
    async fn subscribe(
        &self,
        join_set: &mut JoinSet<()>,
        client: &Arc<SolanaPubsubClient>,
        mention: &Pubkey,
        config: SubscriptionConfig,
    ) -> crate::Result<broadcast::Receiver<WithSlot<RpcLogsResponse>>> {
        let mut map = self.0.write().await;
        loop {
            match map.entry(*mention) {
                Entry::Occupied(entry) => {
                    let subscription = entry.get();
                    if subscription.abort.is_finished() {
                        entry.remove();
                    } else {
                        if config.commitment != subscription.commitment {
                            return Err(crate::Error::unknown(format!(
                                "commitment mismatched, current: {}",
                                subscription.commitment.commitment
                            )));
                        }
                        if let Some(receiver) = subscription.sender.subscribe() {
                            return Ok(receiver);
                        } else {
                            entry.remove();
                        }
                    }
                }
                Entry::Vacant(entry) => {
                    let (sender, receiver) = broadcast::channel(config.capacity.get());
                    let subscription = LogsSubscription::init(
                        join_set,
                        sender.into(),
                        client,
                        mention,
                        config.commitment,
                        config.cleanup_interval,
                    )
                    .await?;
                    entry.insert(subscription);
                    return Ok(receiver);
                }
            }
        }
    }
}

#[derive(Debug)]
struct ClosableSender<T>(Arc<std::sync::RwLock<Option<broadcast::Sender<T>>>>);

impl<T> From<broadcast::Sender<T>> for ClosableSender<T> {
    fn from(sender: broadcast::Sender<T>) -> Self {
        Self(Arc::new(std::sync::RwLock::new(Some(sender))))
    }
}

impl<T> Clone for ClosableSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> ClosableSender<T> {
    fn send(&self, value: T) -> Result<usize, broadcast::error::SendError<T>> {
        match self.0.read().unwrap().as_ref() {
            Some(sender) => sender.send(value),
            None => Err(broadcast::error::SendError(value)),
        }
    }

    fn receiver_count(&self) -> Option<usize> {
        Some(self.0.read().unwrap().as_ref()?.receiver_count())
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<T>> {
        Some(self.0.read().unwrap().as_ref()?.subscribe())
    }

    fn close(&self) -> bool {
        self.0.write().unwrap().take().is_some()
    }
}

impl<T> Drop for ClosableSender<T> {
    fn drop(&mut self) {
        self.close();
    }
}
