use std::{
    collections::{hash_map::Entry, HashMap},
    ops::Deref,
};

use anchor_client::solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use either::Either;
use gmsol_store::states::PriceProviderKind;
use pythnet_sdk::wire::v1::AccumulatorUpdateData;
use time::OffsetDateTime;

use crate::{
    pyth::{EncodingType, Hermes},
    utils::builder::{
        FeedAddressMap, FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
    },
};

use super::{
    hermes::BinaryPriceUpdate, utils, PythPullOracle, PythPullOracleOps, PythReceiverOps,
    WormholeOps, VAA_SPLIT_INDEX,
};

/// Pyth Pull Oracle.
pub struct PythPullOracleWithHermes<'a, C> {
    gmsol: &'a crate::Client<C>,
    hermes: &'a Hermes,
    oracle: &'a PythPullOracle<C>,
}

/// Price updates.
pub struct PriceUpdates {
    num_feeds: Option<usize>,
    updates: Vec<BinaryPriceUpdate>,
}

impl From<Vec<BinaryPriceUpdate>> for PriceUpdates {
    fn from(value: Vec<BinaryPriceUpdate>) -> Self {
        Self {
            num_feeds: None,
            updates: value,
        }
    }
}

impl<'a, C> PythPullOracleWithHermes<'a, C> {
    /// Create from parts.
    pub fn from_parts(
        gmsol: &'a crate::Client<C>,
        hermes: &'a Hermes,
        oracle: &'a PythPullOracle<C>,
    ) -> Self {
        Self {
            gmsol,
            hermes,
            oracle,
        }
    }
}

impl<'a, C> PullOracle for PythPullOracleWithHermes<'a, C> {
    type PriceUpdates = PriceUpdates;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        let feed_ids = utils::extract_pyth_feed_ids(feed_ids)?;
        if feed_ids.is_empty() {
            return Ok(PriceUpdates {
                num_feeds: Some(0),
                updates: vec![],
            });
        }
        let update = self
            .hermes
            .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
            .await?;
        if let Some(after) = after {
            let min_ts = update
                .min_timestamp()
                .ok_or_else(|| crate::Error::invalid_argument("empty price updates"))?;
            let min_ts = OffsetDateTime::from_unix_timestamp(min_ts)
                .map_err(crate::Error::invalid_argument)?;
            if min_ts < after {
                return Err(crate::Error::invalid_argument(format!(
                    "price updates are too old, min_ts={min_ts}, required={after}"
                )));
            }
        }
        Ok(PriceUpdates {
            num_feeds: Some(feed_ids.len()),
            updates: vec![update.binary],
        })
    }
}

impl<'r, 'a, C> PullOracle for &'r PythPullOracleWithHermes<'a, C> {
    type PriceUpdates = PriceUpdates;

    async fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self::PriceUpdates> {
        (*self).fetch_price_updates(feed_ids, after).await
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PostPullOraclePrices<'a, C>
    for PythPullOracleWithHermes<'a, C>
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let mut ixns = PriceUpdateInstructions::new(self.gmsol);

        let PriceUpdates { updates, num_feeds } = price_updates;

        if updates.is_empty() {
            return Ok((ixns, Default::default()));
        }

        let mut prices = HashMap::with_capacity(num_feeds.unwrap_or(0));

        let wormhole = self.oracle.wormhole();
        let pyth = self.oracle.pyth();

        let datas = updates
            .iter()
            .flat_map(
                |update| match utils::parse_accumulator_update_datas(update) {
                    Ok(datas) => Either::Left(datas.into_iter().map(Ok)),
                    Err(err) => Either::Right(std::iter::once(Err(err))),
                },
            )
            .collect::<crate::Result<Vec<AccumulatorUpdateData>>>()?;

        // Merge by ids.
        let mut updates = HashMap::<_, _>::default();
        for data in datas.iter() {
            let proof = &data.proof;
            for update in utils::get_merkle_price_updates(proof) {
                let feed_id = utils::parse_feed_id(update)?;
                updates.insert(feed_id, (proof, update));
            }
        }

        // Write vaas.
        let mut encoded_vaas = HashMap::<_, _>::default();
        let mut vaas = HashMap::<_, _>::default();
        for (proof, _) in updates.values() {
            let vaa = utils::get_vaa_buffer(proof);
            if let Entry::Vacant(entry) = vaas.entry(vaa) {
                let guardian_set_index = utils::get_guardian_set_index(proof)?;

                let mut pubkey: Pubkey;
                loop {
                    let keypair = Keypair::new();
                    pubkey = keypair.pubkey();
                    match encoded_vaas.entry(pubkey) {
                        Entry::Vacant(entry) => {
                            entry.insert(keypair);
                            break;
                        }
                        Entry::Occupied(_) => continue,
                    }
                }

                entry.insert((pubkey, guardian_set_index));
            }
        }

        for (vaa, (pubkey, guardian_set_index)) in vaas.iter() {
            let draft_vaa = encoded_vaas.remove(pubkey).expect("must exist");
            let create = wormhole
                .create_encoded_vaa(draft_vaa, vaa.len() as u64)
                .await?;
            let draft_vaa = pubkey;
            let write_1 = wormhole.write_encoded_vaa(draft_vaa, 0, &vaa[0..VAA_SPLIT_INDEX]);
            let write_2 = wormhole.write_encoded_vaa(
                draft_vaa,
                VAA_SPLIT_INDEX as u32,
                &vaa[VAA_SPLIT_INDEX..],
            );
            let verify = wormhole.verify_encoded_vaa_v1(draft_vaa, *guardian_set_index);
            ixns.try_push_post(create.clear_output())?;
            ixns.try_push_post(write_1)?;
            ixns.try_push_post(write_2)?;
            ixns.try_push_post(verify)?;
            let close_encoded_vaa = wormhole.close_encoded_vaa(draft_vaa);
            ixns.try_push_close(close_encoded_vaa)?;
        }

        // Post price updates.
        for (feed_id, (proof, update)) in updates {
            let price_update = Keypair::new();
            let vaa = utils::get_vaa_buffer(proof);
            let Some((encoded_vaa, _)) = vaas.get(vaa) else {
                continue;
            };
            let (post_price_update, price_update) = pyth
                .post_price_update(price_update, update, encoded_vaa)?
                .swap_output(());
            prices.insert(Pubkey::new_from_array(feed_id.to_bytes()), price_update);
            ixns.try_push_post(post_price_update)?;
            ixns.try_push_close(pyth.reclaim_rent(&price_update))?;
        }

        Ok((ixns, HashMap::from([(PriceProviderKind::Pyth, prices)])))
    }
}

impl<'r, 'a, C: Deref<Target = impl Signer> + Clone> PostPullOraclePrices<'a, C>
    for &'r PythPullOracleWithHermes<'a, C>
where
    'r: 'a,
{
    async fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        (*self).fetch_price_update_instructions(price_updates).await
    }
}
