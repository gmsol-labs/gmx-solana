use gmsol_utils::{oracle::PriceProviderKind, token_config::TokensWithFeed};
use pyth_sdk::Identifier;
use pythnet_sdk::{
    messages::PriceFeedMessage,
    wire::{
        from_slice,
        v1::{AccumulatorUpdateData, MerklePriceUpdate, Proof},
    },
};

use crate::client::feeds_parser::Feeds;

use super::hermes::{BinaryPriceUpdate, EncodingType};

/// Parse [`AccumulatorUpdateData`] from price update.
pub fn parse_accumulator_update_datas(
    update: &BinaryPriceUpdate,
) -> crate::Result<Vec<AccumulatorUpdateData>> {
    let datas = match update.encoding {
        EncodingType::Base64 => {
            use base64::{engine::general_purpose::STANDARD, Engine};

            update
                .data
                .iter()
                .map(|data| {
                    STANDARD
                        .decode(data)
                        .map_err(crate::Error::from)
                        .and_then(|data| parse_accumulator_update_data(&data))
                })
                .collect::<crate::Result<Vec<_>>>()?
        }
        EncodingType::Hex => {
            unimplemented!()
        }
    };
    Ok(datas)
}

#[inline]
fn parse_accumulator_update_data(data: &[u8]) -> crate::Result<AccumulatorUpdateData> {
    AccumulatorUpdateData::try_from_slice(data).map_err(crate::Error::custom)
}

/// Get guardian set index from [`Proof`].
pub fn get_guardian_set_index(proof: &Proof) -> crate::Result<i32> {
    let vaa = get_vaa_buffer(proof);
    if vaa.len() < 5 {
        return Err(crate::Error::custom("invalid vaa"));
    }
    let index: &[u8; 4] = (&vaa[1..5]).try_into().map_err(crate::Error::custom)?;
    Ok(i32::from_be_bytes(*index))
}

/// Get vaa buffer.
pub fn get_vaa_buffer(proof: &Proof) -> &[u8] {
    match proof {
        Proof::WormholeMerkle { vaa, .. } => vaa.as_ref(),
    }
}

/// Get merkle price updates.
pub fn get_merkle_price_updates(proof: &Proof) -> &[MerklePriceUpdate] {
    match proof {
        Proof::WormholeMerkle { updates, .. } => updates,
    }
}

/// Price feed message variant.
pub const PRICE_FEED_MESSAGE_VARIANT: u8 = 0;

/// Parse price feed message.
pub fn parse_price_feed_message(update: &MerklePriceUpdate) -> crate::Result<PriceFeedMessage> {
    const PRICE_FEED_MESSAGE_VARIANT: u8 = 0;
    let data = update.message.as_ref().as_slice();
    if data.is_empty() {
        return Err(crate::Error::custom("empty message"));
    }
    if data[0] != PRICE_FEED_MESSAGE_VARIANT {
        return Err(crate::Error::custom("it is not a price feed message"));
    }
    from_slice::<byteorder::BE, _>(&data[1..])
        .map_err(|err| crate::Error::custom(format!("deserialize price feed message error: {err}")))
}

/// Parse feed id from [`MerklePriceUpdate`].
pub fn parse_feed_id(update: &MerklePriceUpdate) -> crate::Result<Identifier> {
    let feed_id = parse_price_feed_message(update)?.feed_id;
    Ok(Identifier::new(feed_id))
}

/// Extract pyth feed ids from [`TokensWithFeed`].
pub fn extract_pyth_feed_ids(feeds: &TokensWithFeed) -> crate::Result<Vec<Identifier>> {
    Feeds::new(feeds)
        .filter_map(|res| {
            res.map(|config| {
                if matches!(config.provider, PriceProviderKind::Pyth) {
                    Some(Identifier::new(config.feed.to_bytes()))
                } else {
                    None
                }
            })
            .transpose()
        })
        .collect()
}
