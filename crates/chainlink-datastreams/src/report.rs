use std::fmt;

use ruint::aliases::{U192, U256};

/// Report.
pub struct Report {
    /// The stream ID the report has data for.
    pub feed_id: [u8; 32],
    /// Earliest timestamp for which price is applicable.
    pub valid_from_timestamp: u32,
    /// Latest timestamp for which price is applicable.
    pub observations_timestamp: u32,
    native_fee: U192,
    link_fee: U192,
    expires_at: u32,
    // FIXME: the following types should be I192.
    /// DON consensus median price (8 or 18 decimals).
    pub price: U192,
    /// Simulated price impact of a buy order up to the X% depth of liquidity utilisation (8 or 18 decimals).
    pub bid: U192,
    /// Simulated price impact of a sell order up to the X% depth of liquidity utilisation (8 or 18 decimals).
    pub ask: U192,
}

impl Report {
    /// Decimals.
    pub const DECIMALS: u8 = 18;

    /// Get max possible price.
    pub fn max_price() -> U192 {
        (U192::MAX >> 1) - U192::from(1)
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Report")
            .field("feed_id", &self.feed_id)
            .field("valid_from_timestamp", &self.valid_from_timestamp)
            .field("observations_timestamp", &self.observations_timestamp)
            .field("native_fee", self.native_fee.as_limbs())
            .field("link_fee", self.link_fee.as_limbs())
            .field("expires_at", &self.expires_at)
            .field("price", self.price.as_limbs())
            .field("bid", self.bid.as_limbs())
            .field("ask", self.ask.as_limbs())
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
    /// Snap Error.
    #[error(transparent)]
    Snap(#[from] snap::Error),
}

/// Decode compressed report.
pub fn decode_compressed_report(compressed: &[u8]) -> Result<Report, DecodeError> {
    use crate::utils::Compressor;

    let data = Compressor::decompress(compressed)?;

    decode(&data)
}

/// Decode Report.
pub fn decode(mut data: &[u8]) -> Result<Report, DecodeError> {
    let mut offset = 0;

    if data.len() < 3 * 32 {
        return Err(DecodeError::InvalidData);
    }
    offset += 3 * 32;

    // Decode body.
    let dynamic_offset = as_usize(&peek_32_bytes(data, offset)?)?;
    let len = as_usize(&peek_32_bytes(data, dynamic_offset)?)?;
    data = &data[dynamic_offset + 32..(dynamic_offset + 32 + len)];
    offset = 0;

    // Peek version.
    if data.len() < 2 {
        return Err(DecodeError::InvalidData);
    }
    let version = ((data[0] as u16) << 8) | (data[1] as u16);
    if version != 3 {
        return Err(DecodeError::UnsupportedVersion(version));
    }

    // Decode `feed_id`.
    let feed_id = peek_32_bytes(data, offset)?;
    offset += 32;

    // Decode `valid_from_timestamp`.
    let valid_from_timestamp = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `observations_timestamp`.
    let observations_timestamp = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `native_fee`.
    let native_fee = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `link_fee`.
    let link_fee = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `expires_at`.
    let expires_at = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `price`.
    let price = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `price`.
    let bid = as_u256(&peek_32_bytes(data, offset)?);
    offset += 32;

    // Decode `price`.
    let ask = as_u256(&peek_32_bytes(data, offset)?);
    // offset += 32;

    Ok(Report {
        feed_id,
        valid_from_timestamp: valid_from_timestamp
            .try_into()
            .map_err(|_| DecodeError::InvalidData)?,
        observations_timestamp: observations_timestamp
            .try_into()
            .map_err(|_| DecodeError::InvalidData)?,
        native_fee: u256_to_u192(native_fee),
        link_fee: u256_to_u192(link_fee),
        expires_at: expires_at
            .try_into()
            .map_err(|_| DecodeError::InvalidData)?,
        price: u256_to_u192(price),
        bid: u256_to_u192(bid),
        ask: u256_to_u192(ask),
    })
}

fn u256_to_u192(num: U256) -> U192 {
    let inner = num.as_limbs();
    U192::from_limbs([inner[0], inner[1], inner[2]])
}

type Word = [u8; 32];

fn peek(data: &[u8], offset: usize, len: usize) -> Result<&[u8], DecodeError> {
    if offset + len > data.len() {
        Err(DecodeError::InvalidData)
    } else {
        Ok(&data[offset..(offset + len)])
    }
}

fn peek_32_bytes(data: &[u8], offset: usize) -> Result<Word, DecodeError> {
    peek(data, offset, 32).map(|x| {
        let mut out: Word = [0u8; 32];
        out.copy_from_slice(&x[0..32]);
        out
    })
}

fn as_usize(slice: &Word) -> Result<usize, DecodeError> {
    if !slice[..28].iter().all(|x| *x == 0) {
        return Err(DecodeError::InvalidData);
    }
    let result = ((slice[28] as usize) << 24)
        + ((slice[29] as usize) << 16)
        + ((slice[30] as usize) << 8)
        + (slice[31] as usize);
    Ok(result)
}

fn as_u256(slice: &Word) -> U256 {
    U256::from_be_bytes(*slice)
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
        let report = decode(&data).unwrap();
        assert!(report.price == U192::from(1445538218802086900u64));
        assert!(report.bid == U192::from(1445336809268003700u64));
        assert!(report.ask == U192::from(1445876300000000000u64));
        assert_eq!(report.valid_from_timestamp, 1730606208);
        assert_eq!(report.observations_timestamp, 1730606208);
    }
}
