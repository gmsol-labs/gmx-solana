/// Max number of flags for GLV markets.
pub const MAX_GLV_MARKET_FLAGS: usize = 8;

/// GLV Market Config Flag.
#[derive(
    num_enum::IntoPrimitive, Clone, Copy, strum::EnumString, strum::Display, PartialEq, Eq, Hash,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[repr(u8)]
pub enum GlvMarketFlag {
    /// Is deposit allowed.
    IsDepositAllowed,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}
