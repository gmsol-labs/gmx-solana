use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{price::Decimal, token_config::TokenConfig, Price};

/// Max number of oracle flags.
pub const MAX_ORACLE_FLAGS: usize = 8;

/// Oracle error.
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    /// Invalid price feed price.
    #[error("invalid price feed price: {0}")]
    InvalidPriceFeedPrice(&'static str),
}

type OracleResult<T> = std::result::Result<T, OracleError>;

/// Supported Price Provider Kind.
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    Default,
    TryFromPrimitive,
    IntoPrimitive,
    PartialEq,
    Eq,
    Hash,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[non_exhaustive]
pub enum PriceProviderKind {
    /// Chainlink Data Streams.
    #[default]
    ChainlinkDataStreams = 0,
    /// Pyth Oracle V2.
    Pyth = 1,
    /// Chainlink Data Feed.
    Chainlink = 2,
    /// Switchboard On-Demand (V3) Data Feed.
    Switchboard = 3,
}

/// Convert pyth price value with confidence to [`Price`].
pub fn pyth_price_with_confidence_to_price(
    price: i64,
    confidence: u64,
    exponent: i32,
    token_config: &TokenConfig,
) -> OracleResult<Price> {
    let mid_price: u64 = price
        .try_into()
        .map_err(|_| OracleError::InvalidPriceFeedPrice("mid_price"))?;
    // Note: No validation of Pyth’s price volatility has been conducted yet.
    // Exercise caution when choosing Pyth as the primary oracle.
    let min_price = mid_price
        .checked_sub(confidence)
        .ok_or(OracleError::InvalidPriceFeedPrice("min_price"))?;
    let max_price = mid_price
        .checked_add(confidence)
        .ok_or(OracleError::InvalidPriceFeedPrice("max_price"))?;
    Ok(Price {
        min: pyth_price_value_to_decimal(min_price, exponent, token_config)?,
        max: pyth_price_value_to_decimal(max_price, exponent, token_config)?,
    })
}

/// Pyth price value to decimal.
pub fn pyth_price_value_to_decimal(
    mut value: u64,
    exponent: i32,
    token_config: &TokenConfig,
) -> OracleResult<Decimal> {
    // actual price == value * 10^exponent
    // - If `exponent` is not positive, then the `decimals` is set to `-exponent`.
    // - Otherwise, we should use `value * 10^exponent` as `price` argument, and let `decimals` be `0`.
    let decimals: u8 = if exponent <= 0 {
        (-exponent)
            .try_into()
            .map_err(|_| OracleError::InvalidPriceFeedPrice("exponent too small"))?
    } else {
        let factor = 10u64
            .checked_pow(exponent as u32)
            .ok_or(OracleError::InvalidPriceFeedPrice("exponent too big"))?;
        value = value
            .checked_mul(factor)
            .ok_or(OracleError::InvalidPriceFeedPrice("price overflow"))?;
        0
    };
    let price = Decimal::try_from_price(
        value as u128,
        decimals,
        token_config.token_decimals(),
        token_config.precision(),
    )
    .map_err(|_| OracleError::InvalidPriceFeedPrice("converting to Decimal"))?;
    Ok(price)
}

/// Oracle flag.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum OracleFlag {
    /// Cleared.
    Cleared,
    // CHECK: should have no more than `MAX_ORACLE_FLAGS` of flags.
}
