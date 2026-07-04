use num_enum::{IntoPrimitive, TryFromPrimitive};

/// Max number of market-status flags.
pub const MAX_MARKET_STATUS_FLAGS: usize = 8;

/// Market status of a price feed.
///
/// `Disabled` (the zero value) means "no market-status information" — a skip
/// sentinel, neither open nor closed. Persisted on-chain so a future Risk
/// Oracle-driven contract can switch `MarketConfig` sets from a token's status.
#[non_exhaustive]
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum MarketStatus {
    /// No market-status information (skip).
    Disabled = 0,
    /// Unknown.
    Unknown = 1,
    /// Pre-market.
    PreMarket = 2,
    /// Regular trading hours.
    RegularHours = 3,
    /// Post-market.
    PostMarket = 4,
    /// Overnight.
    Overnight = 5,
    /// Closed.
    Closed = 6,
}

/// Per-token market-status policy flags.
///
/// Each flag names the deviation from the default, so all-zero (unconfigured)
/// resolves to: RegularHours open, every other status closed.
///
/// These flags only modify openness on top of the feed's base openness (its
/// `Open` flag plus freshness): a resolved status can close a market whose base
/// is open, but no flag can reopen a base that is already closed (stale, or a
/// report that reported closed).
#[derive(IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
#[non_exhaustive]
pub enum MarketStatusFlag {
    /// Allow trading while the status is Unknown.
    AllowUnknown,
    /// Allow trading during pre-market.
    AllowPreMarket,
    /// Force RegularHours closed. RegularHours is the one status open by
    /// default (all-zero policy), so this is the inverted flag ("Halt", not
    /// "Allow").
    HaltRegularHours,
    /// Allow trading during post-market.
    AllowPostMarket,
    /// Allow trading overnight.
    AllowOvernight,
    /// Allow trading while the status is Closed.
    AllowClosed,
    // CHECK: should have no more than `MAX_MARKET_STATUS_FLAGS` of flags.
    // CHECK: append-only. Each variant's position is a persisted bitmap bit
    // (see `MarketStatusFlagContainer`); only add new flags at the end, never
    // reorder or remove, or stored `TokenConfig` flags will be misread.
}

crate::flags!(MarketStatusFlag, MAX_MARKET_STATUS_FLAGS, u8);

/// Resolved openness of a market status under a per-token policy.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub enum MarketOpenness {
    /// Open.
    Open,
    /// Closed.
    Closed,
    /// No status — defer to the base openness.
    Skip,
}

impl MarketStatus {
    /// Resolve openness under the given per-token flags.
    pub fn openness(self, flags: MarketStatusFlagContainer) -> MarketOpenness {
        let open_if = |open: bool| {
            if open {
                MarketOpenness::Open
            } else {
                MarketOpenness::Closed
            }
        };
        match self {
            Self::Disabled => MarketOpenness::Skip,
            Self::Unknown => open_if(flags.get_flag(MarketStatusFlag::AllowUnknown)),
            Self::PreMarket => open_if(flags.get_flag(MarketStatusFlag::AllowPreMarket)),
            Self::RegularHours => open_if(!flags.get_flag(MarketStatusFlag::HaltRegularHours)),
            Self::PostMarket => open_if(flags.get_flag(MarketStatusFlag::AllowPostMarket)),
            Self::Overnight => open_if(flags.get_flag(MarketStatusFlag::AllowOvernight)),
            Self::Closed => open_if(flags.get_flag(MarketStatusFlag::AllowClosed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> MarketStatusFlagContainer {
        MarketStatusFlagContainer::default()
    }

    #[test]
    fn default_flags_openness() {
        assert!(matches!(
            MarketStatus::Disabled.openness(flags()),
            MarketOpenness::Skip
        ));
        assert!(matches!(
            MarketStatus::RegularHours.openness(flags()),
            MarketOpenness::Open
        ));
        for status in [
            MarketStatus::Unknown,
            MarketStatus::PreMarket,
            MarketStatus::PostMarket,
            MarketStatus::Overnight,
            MarketStatus::Closed,
        ] {
            assert!(matches!(status.openness(flags()), MarketOpenness::Closed));
        }
    }

    #[test]
    fn flags_flip_each_status() {
        let mut halt = flags();
        halt.set_flag(MarketStatusFlag::HaltRegularHours, true);
        assert!(matches!(
            MarketStatus::RegularHours.openness(halt),
            MarketOpenness::Closed
        ));

        for (flag, status) in [
            (MarketStatusFlag::AllowUnknown, MarketStatus::Unknown),
            (MarketStatusFlag::AllowPreMarket, MarketStatus::PreMarket),
            (MarketStatusFlag::AllowPostMarket, MarketStatus::PostMarket),
            (MarketStatusFlag::AllowOvernight, MarketStatus::Overnight),
            (MarketStatusFlag::AllowClosed, MarketStatus::Closed),
        ] {
            let mut f = flags();
            f.set_flag(flag, true);
            assert!(matches!(status.openness(f), MarketOpenness::Open));
        }
    }
}
