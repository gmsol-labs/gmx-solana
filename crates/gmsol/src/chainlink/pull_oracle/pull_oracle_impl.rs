use std::{collections::HashMap, ops::Deref};

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use gmsol_store::states::PriceProviderKind;
use time::OffsetDateTime;

use crate::{
    store::{oracle::OracleOps, utils::Feeds},
    utils::builder::{
        FeedAddressMap, FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
    },
};

use super::{client::ApiReportData, Client, FeedId};

/// Chainlink Pull Oracle Factory.
pub struct ChainlinkPullOracleFactory {
    chainlink_program: Pubkey,
    access_controller: Pubkey,
    store: Pubkey,
    feed_index: u8,
    feeds: FeedAddressMap,
}

impl ChainlinkPullOracleFactory {
    /// Create a new [`ChainlinkPullOracleFactory`] with default program ID and access controller address.
    pub fn new(store: &Pubkey, feed_index: u8) -> Self {
        use chainlink_datastreams::verifier;

        Self::with_program_id_and_access_controller(
            store,
            feed_index,
            &verifier::ID,
            &super::access_controller_address::ID,
        )
    }

    /// Create a new [`ChainlinkPullOracleFactory`] with the given program ID and access controller address.
    pub fn with_program_id_and_access_controller(
        store: &Pubkey,
        feed_index: u8,
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

    /// Prepare feed accounts for the given tokens and feed_ids.
    pub async fn prepare_feeds<C: Deref<Target = impl Signer> + Clone>(
        &mut self,
        gmsol: &crate::Client<C>,
        feed_ids: HashMap<Pubkey, FeedId>,
    ) -> crate::Result<()> {
        let provider = PriceProviderKind::ChainlinkDataStreams;
        let mut txs = gmsol.transaction();
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
                    if *feed.feed_id() != feed_id {
                        return Err(crate::Error::invalid_argument("feed_id mismatched"));
                    }
                }
                None => {
                    txs.push(
                        gmsol
                            .initailize_price_feed(
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
            self.feeds.insert(feed_id, address);
        }

        if !txs.is_emtpy() {
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
        &'a self,
        chainlink: &'a Client,
        gmsol: &'a crate::Client<C>,
    ) -> ChainlinkPullOracle<'a, C> {
        ChainlinkPullOracle::new(chainlink, gmsol, self)
    }
}

/// Chainlink Pull Oracle.
pub struct ChainlinkPullOracle<'a, C> {
    chainlink: &'a Client,
    gmsol: &'a crate::Client<C>,
    ctx: &'a ChainlinkPullOracleFactory,
}

impl<'a, C> ChainlinkPullOracle<'a, C> {
    /// Create a new [`ChainlinkPullOracle`] with default program ID and access controller address.
    pub fn new(
        chainlink: &'a Client,
        gmsol: &'a crate::Client<C>,
        ctx: &'a ChainlinkPullOracleFactory,
    ) -> Self {
        Self {
            chainlink,
            gmsol,
            ctx,
        }
    }
}

impl<'r, 'a, C> PullOracle for &'r ChainlinkPullOracle<'a, C> {
    type PriceUpdates = HashMap<FeedId, ApiReportData>;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        let feed_ids = Feeds::new(feed_ids)
            .filter_map(|res| {
                res.map(|(provider, feed)| {
                    matches!(provider, PriceProviderKind::ChainlinkDataStreams)
                        .then(|| hex::encode(feed.to_bytes()))
                })
                .transpose()
            })
            .collect::<crate::Result<Vec<_>>>()?;

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
                    let ts = OffsetDateTime::from_unix_timestamp(ts)
                        .map_err(crate::Error::invalid_argument)?;
                    if after > ts {
                        return Err(crate::Error::invalid_argument(format!(
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

impl<'r, 'a, C: Deref<Target = impl Signer> + Clone> PostPullOraclePrices<'a, C>
    for &'r ChainlinkPullOracle<'a, C>
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let mut txs = PriceUpdateInstructions::new(self.gmsol);
        let mut map = HashMap::with_capacity(price_updates.len());

        for (feed_id, update) in price_updates {
            let feed_id = Pubkey::new_from_array(*feed_id);
            tracing::info!("adding ix to post price update for {feed_id}");
            let feed = self.ctx.feeds.get(&feed_id).ok_or_else(|| {
                crate::Error::invalid_argument(format!(
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
            txs.try_push_post(rpc)?;
            map.insert(feed_id, *feed);
        }

        Ok((
            txs,
            HashMap::from([(PriceProviderKind::ChainlinkDataStreams, map)]),
        ))
    }
}
