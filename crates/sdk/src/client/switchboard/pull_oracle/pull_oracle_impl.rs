use anchor_spl::associated_token::get_associated_token_address;
use base64::prelude::*;
use gmsol_solana_utils::bundle_builder::BundleOptions;

use anchor_spl::token::spl_token::{native_mint::ID as NATIVE_MINT, ID as SPL_TOKEN_PROGRAM_ID};
use gmsol_utils::oracle::PriceProviderKind;
use rand::Rng;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::AccountMeta, system_program};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use std::{collections::HashMap, num::NonZeroUsize, ops::Deref, sync::Arc};
use switchboard_on_demand_client::{
    fetch_and_cache_luts, oracle_job::OracleJob, prost::Message, CrossbarClient, FeedConfig,
    FetchSignaturesMultiParams, Gateway, MultiSubmission, OracleAccountData, PullFeed,
    PullFeedAccountData, PullFeedSubmitResponseMany, PullFeedSubmitResponseManyParams,
    QueueAccountData, SbContext, SlotHashSysvar, State, SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
};
use time::OffsetDateTime;
use tokio::{join, sync::OnceCell};

use crate::client::feeds_parser::{FeedAddressMap, Feeds};
use crate::client::pull_oracle::{
    FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
};

const DEFAULT_BATCH_SIZE: usize = 5;

const DEVNET_QUEUE: Pubkey = solana_sdk::pubkey!("EYiAmGSdsQTuCw413V5BzaruWuCCSDgTPtBGvLkXHbe7");
#[cfg(not(feature = "devnet"))]
const MAINNET_QUEUE: Pubkey = solana_sdk::pubkey!("A43DyUGA7s8eXPxqEjJY6EBu1KKbNgfxF8h17VAHn13w");

cfg_if::cfg_if! {
    if #[cfg(feature = "devnet")] {
        const QUEUE: Pubkey = DEVNET_QUEUE;
    } else {
        const QUEUE: Pubkey = MAINNET_QUEUE;
    }
}

/// Switchboard Pull Oracle Factory.
#[derive(Debug)]
pub struct SwitchcboardPullOracleFactory {
    switchboard: Pubkey,
    gateways: Vec<Gateway>,
    crossbar: Option<CrossbarClient>,
}

impl SwitchcboardPullOracleFactory {
    /// Gateway Env.
    pub const ENV_GATEWAY: &str = "SWITCHBOARD_GATEWAY";

    /// Create with gateways.
    pub fn from_gateways(gateways: Vec<Gateway>) -> crate::Result<Self> {
        if gateways.is_empty() {
            return Err(crate::Error::custom("switchboard: empty gateway list"));
        }
        Ok(Self {
            switchboard: Pubkey::new_from_array(SWITCHBOARD_ON_DEMAND_PROGRAM_ID.to_bytes()),
            gateways,
            crossbar: None,
        })
    }

    /// Create a new factory from the given gateway url.
    pub fn new(gateway_url: &str) -> Self {
        Self::from_gateways(vec![Gateway::new(gateway_url.to_string())]).expect("must success")
    }

    /// Create from env.
    pub fn from_env() -> crate::Result<Self> {
        use std::env;

        let gateway_url = env::var(Self::ENV_GATEWAY)
            .map_err(|_| crate::Error::custom(format!("{} is not set", Self::ENV_GATEWAY)))?;

        Ok(Self::new(&gateway_url))
    }

    /// Create from default queue.
    pub async fn from_default_queue(client: &RpcClient, testnet: bool) -> crate::Result<Self> {
        let queue = if testnet { DEVNET_QUEUE } else { QUEUE };
        Self::from_queue(client, &queue).await
    }

    /// Create from queue.
    pub async fn from_queue(client: &RpcClient, queue: &Pubkey) -> crate::Result<Self> {
        let queue = QueueAccountData::load(client, queue).await.map_err(|err| {
            crate::Error::custom(format!("switchboard: loading queue data error: {err}"))
        })?;
        let gateways = queue.fetch_gateways(client).await.map_err(|err| {
            crate::Error::custom(format!("switchboard: fetching gateways error: {err}"))
        })?;
        tracing::debug!("loaded {} gateways", gateways.len());

        Self::from_gateways(gateways)
    }

    /// Get the total number of the gateways.
    pub fn num_gateways(&self) -> usize {
        self.gateways.len()
    }

    /// Make an oracle with the gateway index.
    pub fn make_oracle_with_gateway_index<'a, C: Deref<Target = impl Signer> + Clone>(
        &'a self,
        gmsol: &'a crate::Client<C>,
        gateway_index: usize,
    ) -> Option<SwitchboardPullOracle<'a, C>> {
        let gateway = self.gateways.get(gateway_index)?;
        tracing::debug!("using gateway: {gateway:?}");
        Some(SwitchboardPullOracle::from_parts(
            gmsol,
            self.switchboard,
            gateway,
            self.crossbar.clone(),
        ))
    }

    /// Make an oracle with the given rng.
    pub fn make_oracle_with_rng<'a, C: Deref<Target = impl Signer> + Clone>(
        &'a self,
        gmsol: &'a crate::Client<C>,
        rng: &mut impl Rng,
    ) -> SwitchboardPullOracle<'a, C> {
        let index = rng.gen_range(0, self.num_gateways());
        self.make_oracle_with_gateway_index(gmsol, index)
            .expect("must success")
    }

    /// Make an oracle.
    pub fn make_oracle<'a, C: Deref<Target = impl Signer> + Clone>(
        &'a self,
        gmsol: &'a crate::Client<C>,
    ) -> SwitchboardPullOracle<'a, C> {
        let mut rng = rand::thread_rng();
        self.make_oracle_with_rng(gmsol, &mut rng)
    }
}

/// Switchboard Pull Oracle.
pub struct SwitchboardPullOracle<'a, C> {
    gmsol: &'a crate::Client<C>,
    switchboard: Pubkey,
    ctx: Arc<SbContext>,
    client: RpcClient,
    gateway: &'a Gateway,
    crossbar: Option<CrossbarClient>,
    batch_size: usize,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> SwitchboardPullOracle<'a, C> {
    /// Create from parts.
    pub fn from_parts(
        gmsol: &'a crate::Client<C>,
        switchboard: Pubkey,
        gateway: &'a Gateway,
        crossbar: Option<CrossbarClient>,
    ) -> Self {
        Self {
            gmsol,
            switchboard,
            client: gmsol.store_program().rpc(),
            ctx: SbContext::new(),
            gateway,
            crossbar,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    /// Set batch size.
    pub fn set_batch_size(&mut self, batch_size: NonZeroUsize) -> &mut Self {
        self.batch_size = batch_size.get();
        self
    }
}

/// Swtichboard Price Updates type.
pub struct SbPriceUpdates {
    /// The list of feed pubkeys for which prices were fetched.
    pub feeds: Vec<Pubkey>,
    /// The list of price submissions from each oracles.
    pub price_submissions: Vec<MultiSubmission>,
    /// The slot number for which the price updates were signed with the slothash.
    pub slot: u64,
    /// The queue (network) to which all feeds are owned.
    /// This will always be the same unless the network of oracles you use is non-standard.
    pub queue: Pubkey,
    /// The list of oracle pubkeys that signed the price updates.
    pub oracle_keys: Vec<Pubkey>,
}

impl<C: Deref<Target = impl Signer> + Clone> PullOracle for SwitchboardPullOracle<'_, C> {
    type PriceUpdates = Vec<SbPriceUpdates>;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        let feeds = filter_switchboard_feed_ids(feed_ids)?;

        if feeds.is_empty() {
            return Ok(vec![]);
        }

        let mut updates = Vec::new();

        for feeds in feeds.chunks(self.batch_size) {
            let mut num_signatures = 3;
            let mut feed_configs = Vec::new();
            let mut queue = Pubkey::default();

            for feed in feeds {
                tracing::trace!(%feed, "fetching feed data");
                let data = *self
                    .ctx
                    .pull_feed_cache
                    .entry(*feed)
                    .or_insert_with(OnceCell::new)
                    .get_or_try_init(|| PullFeed::load_data(&self.client, feed))
                    .await
                    .map_err(|_| crate::Error::custom("switchboard: fetching job data failed"))?;
                tracing::trace!(%feed, ?data, "fechted feed data");
                let jobs = data
                    .fetch_jobs(&self.crossbar.clone().unwrap_or_default())
                    .await
                    .map_err(|_| crate::Error::custom("switchboard: fetching job data failed"))?;
                tracing::trace!(%feed, ?jobs, "fetched jobs");
                let encoded_jobs = encode_jobs(&jobs);
                let max_variance = (data.max_variance / 1_000_000_000) as u32;
                let min_responses = data.min_responses;
                if min_responses >= num_signatures {
                    num_signatures = min_responses + 1;
                }
                let feed_config = FeedConfig {
                    encoded_jobs,
                    max_variance: Some(max_variance),
                    min_responses: Some(min_responses),
                };
                feed_configs.push(feed_config);
                queue = data.queue;
            }
            let slothash = SlotHashSysvar::get_latest_slothash(&self.client)
                .await
                .map_err(|_| crate::Error::custom("switchboard: fetching slot hash failed"))?;
            let price_signatures = self
                .gateway
                .fetch_signatures_multi(FetchSignaturesMultiParams {
                    recent_hash: Some(slothash.to_base58_hash()),
                    num_signatures: Some(num_signatures),
                    feed_configs,
                    use_timestamp: Some(true),
                })
                .await
                .map_err(|_| crate::Error::custom("switchboard: fetching signatures failed"))?;
            tracing::trace!("fetched price signatures: {price_signatures:#?}");

            let mut all_submissions: Vec<MultiSubmission> = Vec::new();
            let mut oracle_keys = Vec::new();
            for resp in &price_signatures.oracle_responses {
                all_submissions.push(MultiSubmission {
                    values: resp
                        .feed_responses
                        .iter()
                        .map(|x| {
                            if let Some(after) = after {
                                let Some(ts) = x.timestamp else {
                                    return Err(crate::Error::custom(
                                        "switchboard: missing timestamp of the feed result",
                                    ))?;
                                };
                                let ts = OffsetDateTime::from_unix_timestamp(ts)
                                    .map_err(crate::Error::custom)?;
                                if ts < after {
                                    return Err(crate::Error::custom(
                                        "switchboard: feed result is too old, ts={ts}, required={after}",
                                    ));
                                }
                            }
                            Ok(x.success_value.parse().unwrap_or(i128::MAX))
                        })
                        .collect::<crate::Result<Vec<_>>>()?,
                    signature: BASE64_STANDARD
                        .decode(resp.signature.clone())
                        .map_err(|_| crate::Error::custom("switchboard: base64:decode failure"))?
                        .try_into()
                        .map_err(|_| crate::Error::custom("switchboard: signature:decode failure"))?,
                    recovery_id: resp.recovery_id as u8,
                });
                let oracle_key = hex::decode(
                    &resp
                        .feed_responses
                        .first()
                        .ok_or_else(|| crate::Error::custom("switchboard: empty response"))?
                        .oracle_pubkey,
                )
                .map_err(|_| crate::Error::custom("switchboard: hex:decode failure"))?
                .try_into()
                .map_err(|_| crate::Error::custom("switchboard: pubkey:decode failure"))?;
                let oracle_key = Pubkey::new_from_array(oracle_key);
                oracle_keys.push(oracle_key);
            }

            updates.push(SbPriceUpdates {
                feeds: feeds.to_vec(),
                price_submissions: all_submissions,
                slot: slothash.slot,
                queue,
                oracle_keys,
            });
        }

        Ok(updates)
    }
}

impl<'a, C: Clone + Deref<Target = impl Signer>> PostPullOraclePrices<'a, C>
    for SwitchboardPullOracle<'a, C>
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
        options: BundleOptions,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let mut ixns = PriceUpdateInstructions::new(self.gmsol, options);
        let mut prices = HashMap::default();
        for update in price_updates {
            let feeds = &update.feeds;
            let price_signatures = &update.price_submissions;
            let queue = update.queue;
            let oracle_keys = &update.oracle_keys;

            let queue_key = [queue];
            let (oracle_luts_result, pull_feed_luts_result, queue_lut_result) = join!(
                fetch_and_cache_luts::<OracleAccountData>(
                    &self.client,
                    self.ctx.clone(),
                    oracle_keys
                ),
                fetch_and_cache_luts::<PullFeedAccountData>(&self.client, self.ctx.clone(), feeds),
                fetch_and_cache_luts::<QueueAccountData>(
                    &self.client,
                    self.ctx.clone(),
                    &queue_key
                )
            );

            let oracle_luts = oracle_luts_result
                .map_err(|_| crate::Error::custom("switchboard: fetching oracle luts failed"))?;
            let pull_feed_luts = pull_feed_luts_result
                .map_err(|_| crate::Error::custom("switchboard: fetching pull feed luts failed"))?;
            let queue_lut = queue_lut_result
                .map_err(|_| crate::Error::custom("switchboard: fetching queue lut failed"))?;

            let mut luts = oracle_luts;
            luts.extend(pull_feed_luts);
            luts.extend(queue_lut);

            let payer = self.gmsol.payer();

            prices.extend(feeds.iter().map(|feed| (*feed, *feed)));

            let ix_data = PullFeedSubmitResponseManyParams {
                slot: update.slot,
                submissions: price_signatures.clone(),
            };

            let feeds = feeds.iter().map(|pubkey| AccountMeta::new(*pubkey, false));
            let oracles_and_stats = oracle_keys.iter().flat_map(|oracle| {
                let stats_key = OracleAccountData::stats_key(oracle);
                [
                    AccountMeta::new_readonly(*oracle, false),
                    AccountMeta::new(stats_key, false),
                ]
            });
            let ix = self
                .gmsol
                .store_transaction()
                .program(self.switchboard)
                .args(ix_data.data())
                .accounts(
                    PullFeedSubmitResponseMany {
                        queue,
                        program_state: State::key(),
                        recent_slothashes: solana_sdk::sysvar::slot_hashes::ID,
                        payer,
                        system_program: system_program::ID,
                        reward_vault: get_associated_token_address(&queue, &NATIVE_MINT),
                        token_program: SPL_TOKEN_PROGRAM_ID,
                        token_mint: NATIVE_MINT,
                    }
                    .to_account_metas(None),
                )
                .accounts(feeds.chain(oracles_and_stats).collect())
                .lookup_tables(
                    luts.clone()
                        .into_iter()
                        .map(|x| (x.key, x.addresses.clone())),
                );
            ixns.try_push_post(ix).map_err(|(_, err)| err)?;
        }

        Ok((
            ixns,
            HashMap::from([(PriceProviderKind::Switchboard, prices)]),
        ))
    }
}

fn encode_jobs(job_array: &[OracleJob]) -> Vec<String> {
    job_array
        .iter()
        .map(|job| BASE64_STANDARD.encode(job.encode_length_delimited_to_vec()))
        .collect()
}

fn filter_switchboard_feed_ids(feed_ids: &FeedIds) -> crate::Result<Vec<Pubkey>> {
    Feeds::new(feed_ids)
        .filter_map(|res| {
            res.map(|config| {
                matches!(config.provider, PriceProviderKind::Switchboard).then_some(config.feed)
            })
            .transpose()
        })
        .collect()
}
