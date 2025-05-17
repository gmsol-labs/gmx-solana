use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, RwLock},
};

use gmsol_solana_utils::bundle_builder::{BundleBuilder, BundleOptions};

use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use time::OffsetDateTime;

use crate::client::{
    feeds_parser::{FeedAddressMap, Feeds},
    ops::oracle::OracleOps,
    pull_oracle::{FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle},
};

use super::{client::ApiReportData, Client, FeedId};

/// Chainlink Pull Oracle Factory.
pub struct ChainlinkPullOracleFactory {
    chainlink_program: Pubkey,
    access_controller: Pubkey,
    store: Pubkey,
    feed_index: u16,
    feeds: RwLock<FeedAddressMap>,
}

impl ChainlinkPullOracleFactory {
    /// Create a new [`ChainlinkPullOracleFactory`] with default program ID and access controller address.
    pub fn new(store: &Pubkey, feed_index: u16) -> Self {
        use gmsol_chainlink_datastreams::verifier;

        Self::with_program_id_and_access_controller(
            store,
            feed_index,
            &verifier::ID,
            &super::access_controller_address::ID,
        )
    }

    /// Wrap in an [`Arc`].
    pub fn arced(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Create a new [`ChainlinkPullOracleFactory`] with the given program ID and access controller address.
    pub fn with_program_id_and_access_controller(
        store: &Pubkey,
        feed_index: u16,
        chainlink_program: &Pubkey,
        access_controller: &Pubkey,
    ) -> Self {
        Self {
            chainlink_program: *chainlink_program,
            access_controller: *access_controller,
            store: *store,
            feed_index,
            feeds: Default::default(),
        }
    }

    /// Prepare feed accounts but do not send.
    pub async fn prepare_feeds_bundle<'a, C: Deref<Target = impl Signer> + Clone>(
        &self,
        gmsol: &'a crate::Client<C>,
        feed_ids: HashMap<Pubkey, FeedId>,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<'a, C>> {
        let provider = PriceProviderKind::ChainlinkDataStreams;
        let mut txs = gmsol.bundle_with_options(options);
        let authority = gmsol.payer();
        for (token, feed_id) in feed_ids {
            let address = gmsol.find_price_feed_address(
                &self.store,
                &authority,
                self.feed_index,
                provider,
                &token,
            );
            let feed_id = Pubkey::new_from_array(feed_id);
            match gmsol.price_feed(&address).await? {
                Some(feed) => {
                    if feed.feed_id != feed_id {
                        return Err(crate::Error::custom("feed_id mismatched"));
                    }
                }
                None => {
                    txs.push(
                        gmsol
                            .initialize_price_feed(
                                &self.store,
                                self.feed_index,
                                provider,
                                &token,
                                &feed_id,
                            )
                            .0,
                    )?;
                }
            }
            self.feeds.write().unwrap().insert(feed_id, address);
        }

        let feeds = self
            .feeds
            .read()
            .unwrap()
            .values()
            .copied()
            .collect::<Vec<_>>();

        tracing::info!("Using custom feeds: {feeds:#?}");

        Ok(txs)
    }

    /// Prepare feed accounts for the given tokens and feed_ids.
    pub async fn prepare_feeds<C: Deref<Target = impl Signer> + Clone>(
        &self,
        gmsol: &crate::Client<C>,
        feed_ids: HashMap<Pubkey, FeedId>,
    ) -> crate::Result<()> {
        let txs = self
            .prepare_feeds_bundle(gmsol, feed_ids, Default::default())
            .await?;

        if !txs.is_empty() {
            match txs.send_all(false).await {
                Ok(signatures) => {
                    tracing::info!("initialized feeds with txs: {signatures:#?}");
                }
                Err((signatures, err)) => {
                    tracing::error!(%err, "failed to initailize feeds, successful txs: {signatures:#?}");
                }
            }
        }

        Ok(())
    }

    /// Create [`ChainlinkPullOracle`].
    pub fn make_oracle<'a, C>(
        self: Arc<Self>,
        chainlink: &'a Client,
        gmsol: &'a crate::Client<C>,
        skip_feeds_preparation: bool,
    ) -> ChainlinkPullOracle<'a, C> {
        ChainlinkPullOracle::new(chainlink, gmsol, self, skip_feeds_preparation)
    }
}

/// Chainlink Pull Oracle.
pub struct ChainlinkPullOracle<'a, C> {
    chainlink: &'a Client,
    gmsol: &'a crate::Client<C>,
    ctx: Arc<ChainlinkPullOracleFactory>,
    skip_feeds_preparation: bool,
}

impl<C> Clone for ChainlinkPullOracle<'_, C> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            ..*self
        }
    }
}

impl<'a, C> ChainlinkPullOracle<'a, C> {
    /// Create a new [`ChainlinkPullOracle`] with default program ID and access controller address.
    pub fn new(
        chainlink: &'a Client,
        gmsol: &'a crate::Client<C>,
        ctx: Arc<ChainlinkPullOracleFactory>,
        skip_feeds_preparation: bool,
    ) -> Self {
        Self {
            chainlink,
            gmsol,
            ctx,
            skip_feeds_preparation,
        }
    }
}

impl<C: Deref<Target = impl Signer> + Clone> ChainlinkPullOracle<'_, C> {
    /// Prepare feed accounts but do not send.
    pub async fn prepare_feeds_bundle(
        &self,
        feed_ids: &FeedIds,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<C>> {
        self.ctx
            .prepare_feeds_bundle(self.gmsol, filter_feed_ids(feed_ids)?, options)
            .await
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOracle for ChainlinkPullOracle<'_, C> {
    type PriceUpdates = HashMap<FeedId, ApiReportData>;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        let feeds = filter_feed_ids(feed_ids)?;

        let feed_ids = feeds.values().map(hex::encode).collect::<Vec<_>>();

        if !self.skip_feeds_preparation {
            self.ctx.prepare_feeds(self.gmsol, feeds).await?;
        }

        let tasks = feed_ids
            .iter()
            .map(|feed_id| self.chainlink.latest_report(feed_id));
        let price_updates = futures_util::future::try_join_all(tasks).await?;

        let updates = price_updates
            .into_iter()
            .map(|report| {
                let feed_id = report.decode_feed_id()?;
                let ts = report.observations_timestamp;

                if let Some(after) = after {
                    let ts =
                        OffsetDateTime::from_unix_timestamp(ts).map_err(crate::Error::custom)?;
                    if after > ts {
                        return Err(crate::Error::custom(format!(
                            "price updates are too old, ts={ts}, required={after}"
                        )));
                    }
                }

                Ok((feed_id, report.into_data()))
            })
            .collect::<crate::Result<HashMap<_, _>>>()?;

        Ok(updates)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PostPullOraclePrices<'a, C>
    for ChainlinkPullOracle<'a, C>
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
        options: BundleOptions,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let mut txs = PriceUpdateInstructions::new(self.gmsol, options);
        let mut map = HashMap::with_capacity(price_updates.len());

        let feeds = self.ctx.feeds.read().unwrap();
        for (feed_id, update) in price_updates {
            let feed_id = Pubkey::new_from_array(*feed_id);
            tracing::info!("adding ix to post price update for {feed_id}");
            let feed = feeds.get(&feed_id).ok_or_else(|| {
                crate::Error::custom(format!(
                    "feed account for the given `feed_id` is not provided, feed_id = {feed_id}"
                ))
            })?;
            let rpc = self.gmsol.update_price_feed_with_chainlink(
                &self.ctx.store,
                feed,
                &self.ctx.chainlink_program,
                &self.ctx.access_controller,
                &update.report_bytes()?,
            )?;
            txs.try_push_post(rpc).map_err(|(_, err)| err)?;
            map.insert(feed_id, *feed);
        }

        Ok((
            txs,
            HashMap::from([(PriceProviderKind::ChainlinkDataStreams, map)]),
        ))
    }
}

/// Filter feed ids.
pub fn filter_feed_ids(feed_ids: &FeedIds) -> crate::Result<HashMap<Pubkey, FeedId>> {
    Feeds::new(feed_ids)
        .filter_map(|res| {
            res.map(|config| {
                matches!(config.provider, PriceProviderKind::ChainlinkDataStreams)
                    .then(|| (config.token, config.feed.to_bytes()))
            })
            .transpose()
        })
        .collect::<crate::Result<HashMap<_, _>>>()
}
