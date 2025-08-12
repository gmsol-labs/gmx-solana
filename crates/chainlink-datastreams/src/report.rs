use std::fmt;

use num_bigint::{BigInt, BigUint};
use ruint::aliases::U192;

use chainlink_data_streams_report::{
    feed_id::ID,
    report::{
        base::ReportError, v2::ReportDataV2, v3::ReportDataV3, v4::ReportDataV4, v7::ReportDataV7,
        v8::ReportDataV8,
    },
};

type Sign = bool;

type Signed = (Sign, U192);

/// Report.
pub struct Report {
    /// The stream ID the report has data for.
    pub feed_id: ID,
    /// Earliest timestamp for which price is applicable.
    pub valid_from_timestamp: u32,
    /// Latest timestamp for which price is applicable.
    pub observations_timestamp: u32,
    last_update_timestamp: Option<u64>,
    native_fee: U192,
    link_fee: U192,
    expires_at: u32,
    /// DON consensus median price (8 or 18 decimals).
    price: Signed,
    /// Simulated price impact of a buy order up to the X% depth of liquidity utilisation (8 or 18 decimals).
    bid: Signed,
    /// Simulated price impact of a sell order up to the X% depth of liquidity utilisation (8 or 18 decimals).
    ask: Signed,
    market_status: MarketStatus,
}

/// Market status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketStatus {
    /// Unknown.
    Unknown,
    /// Closed.
    Closed,
    /// Open.
    Open,
}

impl Report {
    /// Decimals.
    pub const DECIMALS: u8 = 18;

    const WORD_SIZE: usize = 32;

    /// Get non-negative price.
    pub fn non_negative_price(&self) -> Option<U192> {
        non_negative(self.price)
    }

    /// Get non-negative bid.
    pub fn non_negative_bid(&self) -> Option<U192> {
        non_negative(self.bid)
    }

    /// Get non-negative ask.
    pub fn non_negative_ask(&self) -> Option<U192> {
        non_negative(self.ask)
    }

    /// Returns the market status.
    pub fn market_status(&self) -> MarketStatus {
        self.market_status
    }

    /// Returns timestamp of the last valid price update, in **nanoseconds**.
    ///
    /// Returns `None` if the feature is not enabled and the value should be ignored.
    pub fn last_update_timestamp(&self) -> Option<u64> {
        self.last_update_timestamp
    }

    /// Returns the expiry timestamp for the report.
    pub fn expires_at(&self) -> u32 {
        self.expires_at
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Report")
            .field("feed_id", &self.feed_id)
            .field("valid_from_timestamp", &self.valid_from_timestamp)
            .field("observations_timestamp", &self.observations_timestamp)
            .field("last_update_timestamp", &self.last_update_timestamp)
            .field("native_fee", self.native_fee.as_limbs())
            .field("link_fee", self.link_fee.as_limbs())
            .field("expires_at", &self.expires_at)
            .field("price", self.price.1.as_limbs())
            .field("bid", self.bid.1.as_limbs())
            .field("ask", self.ask.1.as_limbs())
            .field("market_status", &self.market_status)
            .finish()
    }
}

/// Decode Report Error.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Invalid data.
    #[error("invalid data")]
    InvalidData,
    /// Unsupported Version.
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u16),
    /// Overflow.
    #[error("num overflow")]
    NumOverflow,
    /// Negative value.
    #[error("negative value")]
    NegativeValue,
    /// Snap Error.
    #[error(transparent)]
    Snap(#[from] snap::Error),
    /// Report.
    #[error(transparent)]
    Report(#[from] chainlink_data_streams_report::report::base::ReportError),
}

/// Decode compressed full report.
pub fn decode_compressed_full_report(compressed: &[u8]) -> Result<Report, DecodeError> {
    use crate::utils::Compressor;

    let data = Compressor::decompress(compressed)?;

    let (_, blob) = decode_full_report(&data)?;
    decode(blob)
}

/// Decode Report.
pub fn decode(data: &[u8]) -> Result<Report, DecodeError> {
    let feed_id = decode_feed_id(data)?;
    let version = decode_version(&feed_id);

    match version {
        2 => {
            let report = ReportDataV2::decode(data)?;
            let price = bigint_to_signed(report.benchmark_price)?;
            Ok(Report {
                feed_id: report.feed_id,
                valid_from_timestamp: report.valid_from_timestamp,
                observations_timestamp: report.observations_timestamp,
                last_update_timestamp: None,
                native_fee: bigint_to_u192(report.native_fee)?,
                link_fee: bigint_to_u192(report.link_fee)?,
                expires_at: report.expires_at,
                price,
                // Bid and ask values are not available for the report schema v2.
                bid: price,
                ask: price,
                market_status: MarketStatus::Open,
            })
        }
        3 => {
            let report = ReportDataV3::decode(data)?;
            Ok(Report {
                feed_id: report.feed_id,
                valid_from_timestamp: report.valid_from_timestamp,
                observations_timestamp: report.observations_timestamp,
                last_update_timestamp: None,
                native_fee: bigint_to_u192(report.native_fee)?,
                link_fee: bigint_to_u192(report.link_fee)?,
                expires_at: report.expires_at,
                price: bigint_to_signed(report.benchmark_price)?,
                bid: bigint_to_signed(report.bid)?,
                ask: bigint_to_signed(report.ask)?,
                market_status: MarketStatus::Open,
            })
        }
        4 => {
            let report = ReportDataV4::decode(data)?;
            let price = bigint_to_signed(report.price)?;
            Ok(Report {
                feed_id: report.feed_id,
                valid_from_timestamp: report.valid_from_timestamp,
                observations_timestamp: report.observations_timestamp,
                last_update_timestamp: None,
                native_fee: bigint_to_u192(report.native_fee)?,
                link_fee: bigint_to_u192(report.link_fee)?,
                expires_at: report.expires_at,
                price,
                // Bid and ask values are not available for the first iteration
                // of the RWA report schema (v4).
                bid: price,
                ask: price,
                market_status: decode_market_status(report.market_status)?,
            })
        }
        7 => {
            let report = ReportDataV7::decode(data)?;
            let price = bigint_to_signed(report.exchange_rate)?;
            Ok(Report {
                feed_id: report.feed_id,
                valid_from_timestamp: report.valid_from_timestamp,
                observations_timestamp: report.observations_timestamp,
                last_update_timestamp: None,
                native_fee: bigint_to_u192(report.native_fee)?,
                link_fee: bigint_to_u192(report.link_fee)?,
                expires_at: report.expires_at,
                price,
                // Bid and ask values are not available for the report schema v7.
                bid: price,
                ask: price,
                market_status: MarketStatus::Open,
            })
        }
        8 => {
            let report = ReportDataV8::decode(data)?;
            let price = bigint_to_signed(report.mid_price)?;
            Ok(Report {
                feed_id: report.feed_id,
                valid_from_timestamp: report.valid_from_timestamp,
                observations_timestamp: report.observations_timestamp,
                last_update_timestamp: Some(report.last_update_timestamp),
                native_fee: bigint_to_u192(report.native_fee)?,
                link_fee: bigint_to_u192(report.link_fee)?,
                expires_at: report.expires_at,
                price,
                bid: price,
                ask: price,
                market_status: decode_market_status(report.market_status)?,
            })
        }
        version => Err(DecodeError::UnsupportedVersion(version)),
    }
}

fn decode_feed_id(data: &[u8]) -> Result<ID, DecodeError> {
    if data.len() < Report::WORD_SIZE {
        return Err(ReportError::DataTooShort("feed_id").into());
    }
    let feed_id = ID(data[..Report::WORD_SIZE]
        .try_into()
        .map_err(|_| ReportError::InvalidLength("feed_id (bytes32)"))?);
    Ok(feed_id)
}

fn decode_version(id: &ID) -> u16 {
    // This implementation is based on the `chainlink-data-streams-sdk`:
    // https://docs.rs/chainlink-data-streams-sdk/1.0.0/chainlink_data_streams_sdk/feed/struct.Feed.html#method.version
    u16::from_be_bytes((&id.0[0..2]).try_into().unwrap())
}

fn bigint_to_u192(num: BigInt) -> Result<U192, DecodeError> {
    let Some(num) = num.to_biguint() else {
        return Err(DecodeError::NegativeValue);
    };
    biguint_to_u192(num)
}

fn biguint_to_u192(num: BigUint) -> Result<U192, DecodeError> {
    let mut iter = num.iter_u64_digits();
    if iter.len() > 3 {
        return Err(DecodeError::InvalidData);
    }

    let ans = U192::from_limbs([
        iter.next().unwrap_or_default(),
        iter.next().unwrap_or_default(),
        iter.next().unwrap_or_default(),
    ]);
    Ok(ans)
}

fn bigint_to_signed(num: BigInt) -> Result<Signed, DecodeError> {
    let (sign, num) = num.into_parts();
    let sign = !matches!(sign, num_bigint::Sign::Minus);
    Ok((sign, biguint_to_u192(num)?))
}

fn non_negative(num: Signed) -> Option<U192> {
    match num.0 {
        true => Some(num.1),
        false => None,
    }
}

fn decode_market_status(market_status: u32) -> Result<MarketStatus, DecodeError> {
    match market_status {
        0 => Ok(MarketStatus::Unknown),
        1 => Ok(MarketStatus::Closed),
        2 => Ok(MarketStatus::Open),
        _ => Err(DecodeError::InvalidData),
    }
}

/// Decode full report.
pub fn decode_full_report(payload: &[u8]) -> Result<([[u8; 32]; 3], &[u8]), ReportError> {
    if payload.len() < 128 {
        return Err(ReportError::DataTooShort("Payload is too short"));
    }

    // Decode the first three bytes32 elements
    let mut report_context: [[u8; 32]; 3] = Default::default();
    for idx in 0..3 {
        let context = payload[idx * Report::WORD_SIZE..(idx + 1) * Report::WORD_SIZE]
            .try_into()
            .map_err(|_| ReportError::ParseError("report_context"))?;
        report_context[idx] = context;
    }

    // Decode the offset for the bytes reportBlob data
    let offset = usize::from_be_bytes(
        payload[96..128][24..Report::WORD_SIZE] // Offset value is stored as Little Endian
            .try_into()
            .map_err(|_| ReportError::ParseError("offset as usize"))?,
    );

    if offset < 128 || offset >= payload.len() {
        return Err(ReportError::InvalidLength("offset"));
    }

    // Decode the length of the bytes reportBlob data
    let length = usize::from_be_bytes(
        payload[offset..offset + 32][24..Report::WORD_SIZE] // Length value is stored as Little Endian
            .try_into()
            .map_err(|_| ReportError::ParseError("length as usize"))?,
    );

    if offset + Report::WORD_SIZE + length > payload.len() {
        return Err(ReportError::InvalidLength("bytes data"));
    }

    // Decode the remainder of the payload (actual bytes reportBlob data)
    let report_blob = &payload[offset + Report::WORD_SIZE..offset + Report::WORD_SIZE + length];

    Ok((report_context, report_blob))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode() {
        let data = hex::decode(
            "\
        0006f3dad14cf5df26779bd7b940cd6a9b50ee226256194abbb7643655035d6f\
        0000000000000000000000000000000000000000000000000000000037a8ac19\
        0000000000000000000000000000000000000000000000000000000000000000\
        00000000000000000000000000000000000000000000000000000000000000e0\
        0000000000000000000000000000000000000000000000000000000000000220\
        0000000000000000000000000000000000000000000000000000000000000280\
        0101000000000000000000000000000000000000000000000000000000000000\
        0000000000000000000000000000000000000000000000000000000000000120\
        000305a183fedd7f783d99ac138950cff229149703d2a256d61227ad1e5e66ea\
        000000000000000000000000000000000000000000000000000000006726f480\
        000000000000000000000000000000000000000000000000000000006726f480\
        0000000000000000000000000000000000000000000000000000251afa5b7860\
        000000000000000000000000000000000000000000000000002063f8083c6714\
        0000000000000000000000000000000000000000000000000000000067284600\
        000000000000000000000000000000000000000000000000140f9559e8f303f4\
        000000000000000000000000000000000000000000000000140ede2b99374374\
        0000000000000000000000000000000000000000000000001410c8d592a7f800\
        0000000000000000000000000000000000000000000000000000000000000002\
        abc5fcd50a149ad258673b44c2d1737d175c134a29ab0e1091e1f591af564132\
        737fedd8929a5e6ee155532f116946351e79c1ea3efdb3c88792f48c7cbb02ca\
        0000000000000000000000000000000000000000000000000000000000000002\
        7a478e131ba1474e6b53f2c626ec349f27d64606b1e783d7cb637568ad3b0f7c\
        3ed29f3fd7de70dc2b08e010ab93448e7dd423047e0f224d7145e0489faa9f23",
        )
        .unwrap();
        let (_, data) = decode_full_report(&data).unwrap();
        let report = decode(data).unwrap();
        println!("{report:?}");
        assert!(report.price == (true, U192::from(1445538218802086900u64)));
        assert!(report.bid == (true, U192::from(1445336809268003700u64)));
        assert!(report.ask == (true, U192::from(1445876300000000000u64)));
        assert_eq!(report.valid_from_timestamp, 1730606208);
        assert_eq!(report.observations_timestamp, 1730606208);
    }
}
