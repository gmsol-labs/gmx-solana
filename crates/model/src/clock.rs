/// The kind of clocks.
#[derive(Debug, Clone, Copy, num_enum::TryFromPrimitive, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "strum",
    derive(strum::EnumIter, strum::EnumString, strum::Display)
)]
#[cfg_attr(feature = "strum", strum(serialize_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[repr(u8)]
#[non_exhaustive]
pub enum ClockKind {
    /// Price Impact Distribution.
    PriceImpactDistribution,
    /// Borrowing.
    Borrowing,
    /// Funding.
    Funding,
    /// ADL updated clock for long.
    AdlForLong,
    /// ADL updated clock for short.
    AdlForShort,
}
