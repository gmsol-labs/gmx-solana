use num_enum::{IntoPrimitive, TryFromPrimitive};

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
