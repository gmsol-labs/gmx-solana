use std::{
    collections::{hash_map::Entry, HashMap},
    ops::Deref,
};

use either::Either;
use gmsol_solana_utils::bundle_builder::BundleOptions;
use gmsol_utils::oracle::PriceProviderKind;
use pythnet_sdk::wire::v1::AccumulatorUpdateData;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use time::OffsetDateTime;

use crate::client::{
    feeds_parser::FeedAddressMap,
    pull_oracle::{FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle},
};

use super::{
    hermes::{BinaryPriceUpdate, EncodingType, Hermes},
    utils, PythPullOracle, PythReceiverOps, WormholeOps, VAA_SPLIT_INDEX,
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

impl<C> PullOracle for PythPullOracleWithHermes<'_, C> {
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
                .ok_or_else(|| crate::Error::custom("empty price updates"))?;
            let min_ts =
                OffsetDateTime::from_unix_timestamp(min_ts).map_err(crate::Error::custom)?;
            if min_ts < after {
                return Err(crate::Error::custom(format!(
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

impl<C> PullOracle for &PythPullOracleWithHermes<'_, C> {
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
        options: BundleOptions,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        let mut ixns = PriceUpdateInstructions::new(self.gmsol, options);

        let PriceUpdates { updates, num_feeds } = price_updates;

        if updates.is_empty() {
            return Ok((ixns, Default::default()));
        }

        let mut prices = HashMap::with_capacity(num_feeds.unwrap_or(0));

        let wormhole = &self.oracle.wormhole;
        let pyth = &self.oracle.pyth;

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
            // The split exists because a VAA can exceed what one transaction can carry, which is
            // not a property of every VAA: the length scales with the guardian signature count,
            // and Hermes currently serves 292-byte VAAs at 3 signatures (measured 2026-09-07, by
            // then on guardian set 1). The set index does not enter the length, the signature
            // count does, so a rotation alone leaves this untouched. Slicing at a fixed index
            // panics on anything shorter than it, so only split when there is something to split.
            let split = (vaa.len() > VAA_SPLIT_INDEX).then_some(VAA_SPLIT_INDEX);
            let write_1 =
                wormhole.write_encoded_vaa(draft_vaa, 0, &vaa[0..split.unwrap_or(vaa.len())]);
            let write_2 =
                split.map(|at| wormhole.write_encoded_vaa(draft_vaa, at as u32, &vaa[at..]));
            let verify = wormhole.verify_encoded_vaa_v1(draft_vaa, *guardian_set_index);
            ixns.try_push_post(create.clear_output())
                .map_err(|(_, err)| err)?;
            ixns.try_push_post(write_1).map_err(|(_, err)| err)?;
            if let Some(write_2) = write_2 {
                ixns.try_push_post(write_2).map_err(|(_, err)| err)?;
            }
            ixns.try_push_post(verify).map_err(|(_, err)| err)?;
            let close_encoded_vaa = wormhole.close_encoded_vaa(draft_vaa);
            ixns.try_push_close(close_encoded_vaa)
                .map_err(|(_, err)| err)?;
            ixns.push_temporary_account(*draft_vaa);
        }

        // Post price updates.
        let (post, close) = ixns.split_mut();
        {
            let mut post = post.push_parallel();
            let mut close = close.push_parallel();
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
                post.add(post_price_update);
                close.add(pyth.reclaim_rent(&price_update));
            }
        }
        for price_update in prices.values() {
            ixns.push_temporary_account(*price_update);
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
        options: BundleOptions,
    ) -> crate::Result<(
        PriceUpdateInstructions<'a, C>,
        HashMap<PriceProviderKind, FeedAddressMap>,
    )> {
        (*self)
            .fetch_price_update_instructions(price_updates, options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::Arc,
    };

    use base64::{engine::general_purpose::STANDARD, Engine};
    use pythnet_sdk::{
        accumulators::merkle::MerklePath,
        messages::PriceFeedMessage,
        wire::{
            to_vec,
            v1::{MerklePriceUpdate, Proof},
        },
    };

    use super::*;
    use crate::client::pyth::pull_oracle::utils::PRICE_FEED_MESSAGE_VARIANT;

    /// Serves JSON-RPC on localhost, answering every request with `result: 1_000_000`.
    /// `fetch_price_update_instructions` only asks for the rent-exempt balance.
    fn serve_rent_rpc() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut reader = BufReader::new(stream.unwrap());
                // One connection carries several requests (keep-alive).
                while let Some(body) = read_http_body(&mut reader) {
                    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": 1_000_000,
                        "id": request["id"],
                    })
                    .to_string();
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{response}",
                        response.len()
                    );
                    reader.get_mut().write_all(reply.as_bytes()).unwrap();
                }
            }
        });
        url
    }

    /// Reads one HTTP request and returns its body, or `None` when the connection closes.
    fn read_http_body(reader: &mut impl BufRead) -> Option<Vec<u8>> {
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().ok()?;
                }
            }
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).ok()?;
        Some(body)
    }

    /// A price feed message for `feed_id`, as Hermes puts it in a Merkle price update.
    fn price_update(feed_id: [u8; 32]) -> MerklePriceUpdate {
        let message = PriceFeedMessage {
            feed_id,
            price: 100,
            conf: 1,
            exponent: -2,
            publish_time: 1,
            prev_publish_time: 0,
            ema_price: 100,
            ema_conf: 1,
        };
        let mut bytes = vec![PRICE_FEED_MESSAGE_VARIANT];
        bytes.extend(to_vec::<_, byteorder::BE>(&message).unwrap());
        MerklePriceUpdate {
            message: bytes.into(),
            proof: MerklePath::default(),
        }
    }

    /// Two feeds proven by one VAA: Pyth creates one encoded VAA and two price update accounts.
    #[tokio::test]
    async fn records_encoded_vaa_and_price_update_accounts() {
        let cluster = serve_rent_rpc().parse().unwrap();
        let client = crate::Client::new(cluster, Arc::new(Keypair::new())).unwrap();
        let hermes = Hermes::default();
        let pyth = PythPullOracle::try_new(&client).unwrap();
        let oracle = PythPullOracleWithHermes::from_parts(&client, &hermes, &pyth);

        // Version 1, guardian set 0; the rest of the VAA is not parsed by the SDK.
        let mut vaa = vec![1, 0, 0, 0, 0];
        vaa.resize(100, 0);
        let data = AccumulatorUpdateData::new(Proof::WormholeMerkle {
            vaa: vaa.into(),
            updates: vec![price_update([1; 32]), price_update([2; 32])],
        });
        let encoded = STANDARD.encode(to_vec::<_, byteorder::BE>(&data).unwrap());
        let updates = PriceUpdates::from(vec![BinaryPriceUpdate {
            encoding: EncodingType::Base64,
            data: vec![encoded],
        }]);

        let (ixns, feeds) = oracle
            .fetch_price_update_instructions(&updates, Default::default())
            .await
            .unwrap();

        let price_updates = &feeds[&PriceProviderKind::Pyth];
        let recorded = ixns.temporary_accounts();
        assert_eq!(price_updates.len(), 2);
        assert_eq!(recorded.len(), 3, "one encoded VAA and two price updates");
        assert!(price_updates.values().all(|p| recorded.contains(p)));
        let encoded_vaa = recorded
            .iter()
            .find(|account| !price_updates.values().any(|p| p == *account));
        assert!(encoded_vaa.is_some());
    }
}
