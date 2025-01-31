use crate::{
    store::utils::Feeds,
    utils::builder::{
        FeedAddressMap, FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
    },
};
use anchor_client::solana_client::nonblocking::rpc_client::RpcClient;
use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use anchor_spl::associated_token::get_associated_token_address;
use base64::prelude::*;
use gmsol_solana_utils::program::Program;
use gmsol_store::states::PriceProviderKind;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    system_program,
};
use spl_token::{native_mint::ID as NATIVE_MINT, ID as SPL_TOKEN_PROGRAM_ID};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};
use switchboard_on_demand_client::{
    fetch_and_cache_luts, oracle_job::OracleJob, prost::Message, BatchFeedRequest, CrossbarClient,
    FetchSignaturesBatchParams, Gateway, OracleAccountData, PullFeed, PullFeedAccountData,
    PullFeedSubmitResponse, PullFeedSubmitResponseParams, QueueAccountData, SbContext,
    SlotHashSysvar, State, Submission, SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
};
use time::OffsetDateTime;
use tokio::{join, sync::OnceCell};

/// Switchboard Pull Oracle.
pub struct SwitchboardPullOracle<'a, C> {
    gmsol: &'a crate::Client<C>,
    switchboard: &'a Program<C>,
    ctx: Arc<SbContext>,
    client: RpcClient,
    gateway: Gateway,
    crossbar: Option<CrossbarClient>,
}
impl<'a, C> SwitchboardPullOracle<'a, C> {
    /// Create from parts.
    pub fn from_parts(
        gmsol: &'a crate::Client<C>,
        switchboard: &'a Program<C>,
        gateway: Gateway,
        crossbar: Option<CrossbarClient>,
    ) -> Self {
        Self {
            gmsol,
            switchboard,
            client: switchboard.rpc(),
            ctx: SbContext::new(),
            gateway,
            crossbar,
        }
    }
}

/// Swtichboard Price Updates type.
pub struct SbPriceUpdates {
    /// The list of feed pubkeys for which prices were fetched.
    pub feeds: Vec<Pubkey>,
    /// The list of price submissions for each feed.
    /// The outer index corresponds to the feed index in `feeds`.
    /// The inner index corresponds to the oracle index.
    pub price_submissions: Vec<Vec<Submission>>,
    /// The slot number for which the price updates were signed with the slothash.
    pub slot: u64,
    /// The queue (network) to which all feeds are owned.
    /// This will always be the same unless the network of oracles you use is non-standard.
    pub queue: Pubkey,
    /// The list of oracle pubkeys that signed the price updates.
    pub oracle_keys: Vec<Pubkey>,
}

impl<C: Deref<Target = impl Signer> + Clone> PullOracle for SwitchboardPullOracle<'_, C> {
    type PriceUpdates = SbPriceUpdates;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        let feeds = filter_switchboard_feed_ids(feed_ids)?;

        if feeds.is_empty() {
            return Err(crate::Error::switchboard_error("no switchboard feed found"));
        }

        let mut num_signatures = 3;
        let mut feed_configs = Vec::new();
        let mut queue = Pubkey::default();

        for feed in &feeds {
            let data = *self
                .ctx
                .pull_feed_cache
                .entry(*feed)
                .or_insert_with(OnceCell::new)
                .get_or_try_init(|| PullFeed::load_data(&self.client, feed))
                .await
                .map_err(|_| crate::Error::switchboard_error("fetching job data failed"))?;
            let jobs = data
                .fetch_jobs(&self.crossbar.clone().unwrap_or_default())
                .await
                .map_err(|_| crate::Error::switchboard_error("fetching job data failed"))?;
            let encoded_jobs = encode_jobs(&jobs);
            let max_variance = data.max_variance / 1_000_000_000;
            let min_responses = data.min_responses;
            if min_responses >= num_signatures {
                num_signatures = min_responses + 1;
            }
            let feed_config = BatchFeedRequest {
                jobs_b64_encoded: encoded_jobs,
                max_variance,
                min_responses,
            };
            feed_configs.push(feed_config);
            queue = data.queue;
        }
        let slothash = SlotHashSysvar::get_latest_slothash(&self.client)
            .await
            .map_err(|_| crate::Error::switchboard_error("fetching slot hash failed"))?;
        let price_signatures = self
            .gateway
            .fetch_signatures_batch(FetchSignaturesBatchParams {
                recent_hash: Some(slothash.to_base58_hash()),
                num_signatures: Some(num_signatures),
                feed_configs,
                ..Default::default()
            })
            .await
            .map_err(|_| crate::Error::switchboard_error("fetching signatures failed"))?;

        let mut all_submissions: Vec<Vec<Submission>> = vec![Default::default(); feeds.len()];
        let mut oracle_keys: HashSet<Pubkey> = HashSet::new();
        for resp in &price_signatures.oracle_responses {
            for x in &resp.feed_responses {
                let oracle_key = hex::decode(&x.oracle_pubkey)
                    .map_err(|_| crate::Error::switchboard_error("hex:decode failure"))?
                    .try_into()
                    .map_err(|_| crate::Error::switchboard_error("pubkey:decode failure"))?;
                let oracle_key = Pubkey::new_from_array(oracle_key);
                oracle_keys.insert(oracle_key);
            }
            for (idx, x) in resp.feed_responses.iter().enumerate() {
                if let Some(after) = after {
                    let Some(ts) = x.timestamp else {
                        return Err(crate::Error::switchboard_error(
                            "missing timestamp of the feed result",
                        ))?;
                    };
                    let ts = OffsetDateTime::from_unix_timestamp(ts)
                        .map_err(crate::Error::switchboard_error)?;
                    if ts < after {
                        return Err(crate::Error::switchboard_error(
                            "feed result is too old, ts={ts}, required={after}",
                        ));
                    }
                }

                let mut value_i128 = i128::MAX;
                if let Ok(val) = x.success_value.parse::<i128>() {
                    value_i128 = val;
                }
                all_submissions[idx].push(Submission {
                    value: value_i128,
                    signature: BASE64_STANDARD
                        .decode(x.signature.clone())
                        .map_err(|_| crate::Error::switchboard_error("base64:decode failure"))?
                        .try_into()
                        .map_err(|_| crate::Error::switchboard_error("signature:decode failure"))?,
                    recovery_id: x.recovery_id as u8,
                    offset: 0,
                });
            }
        }
        Ok(SbPriceUpdates {
            feeds: feeds.clone(),
            price_submissions: all_submissions,
            slot: slothash.slot,
            queue,
            oracle_keys: oracle_keys.into_iter().collect::<Vec<_>>(),
        })
    }
}

impl<'a, C: Clone + Deref<Target = impl Signer>> PostPullOraclePrices<'a, C>
    for SwitchboardPullOracle<'a, C>
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let feeds = price_updates.feeds.clone();
        let price_signatures = &price_updates.price_submissions;
        let queue = price_updates.queue;
        let oracle_keys = &price_updates.oracle_keys;

        let queue_key = [queue];
        let (oracle_luts_result, pull_feed_luts_result, queue_lut_result) = join!(
            fetch_and_cache_luts::<OracleAccountData>(&self.client, self.ctx.clone(), oracle_keys),
            fetch_and_cache_luts::<PullFeedAccountData>(&self.client, self.ctx.clone(), &feeds),
            fetch_and_cache_luts::<QueueAccountData>(&self.client, self.ctx.clone(), &queue_key)
        );

        let oracle_luts = oracle_luts_result
            .map_err(|_| crate::Error::switchboard_error("fetching oracle luts failed"))?;
        let pull_feed_luts = pull_feed_luts_result
            .map_err(|_| crate::Error::switchboard_error("fetching pull feed luts failed"))?;
        let queue_lut = queue_lut_result
            .map_err(|_| crate::Error::switchboard_error("fetching queue lut failed"))?;

        let mut luts = oracle_luts;
        luts.extend(pull_feed_luts);
        luts.extend(queue_lut);
        let mut ixns = PriceUpdateInstructions::new(self.gmsol);

        let payer = self.gmsol.payer();
        let mut prices = HashMap::<Pubkey, Pubkey>::new();
        for (idx, submissions) in price_signatures.iter().enumerate() {
            let feed = feeds[idx];
            prices.insert(feed, feed);
            let ix_data = PullFeedSubmitResponseParams {
                slot: price_updates.slot,
                submissions: submissions.to_vec(),
            };
            let mut remaining_accounts = Vec::new();
            for oracle in oracle_keys.iter() {
                remaining_accounts.push(AccountMeta::new_readonly(*oracle, false));
                let stats_key = OracleAccountData::stats_key(oracle);
                remaining_accounts.push(AccountMeta::new(stats_key, false));
            }
            let mut submit_ix = Instruction {
                program_id: *SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
                data: ix_data.data(),
                accounts: PullFeedSubmitResponse {
                    feed,
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
            };
            submit_ix.accounts.extend(remaining_accounts);
            let ix = self
                .switchboard
                .transaction()
                .pre_instruction(submit_ix)
                .lookup_tables(
                    luts.clone()
                        .into_iter()
                        .map(|x| (x.key, x.addresses.clone())),
                );
            ixns.try_push_post(ix)?;
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
