use gmsol_utils::price::market_status::MarketStatus as FeedMarketStatus;
use gmsol_utils::price::{feed_price::PriceFeedPrice, find_divisor_decimals, PriceFlag, TEN, U192};

use crate::report::ExtendedMarketStatus;
use crate::{report::MarketStatus, Report};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

impl super::FromChainlinkReport for PriceFeedPrice {
    fn from_chainlink_report(report: &Report) -> Result<Self, crate::Error> {
        let price = report
            .non_negative_price()
            .ok_or(crate::Error::NegativePrice("price"))?;
        let bid = report
            .non_negative_bid()
            .ok_or(crate::Error::NegativePrice("bid"))?;
        let ask = report
            .non_negative_ask()
            .ok_or(crate::Error::NegativePrice("ask"))?;

        if ask < price {
            return Err(crate::Error::InvalidRange("ask < price"));
        }
        if price < bid {
            return Err(crate::Error::InvalidRange("price < bid"));
        }

        let divisor_decimals = find_divisor_decimals(&ask);

        if Report::DECIMALS < divisor_decimals {
            return Err(crate::Error::Overflow("divisor_decimals"));
        }

        let divisor = TEN.pow(U192::from(divisor_decimals));

        debug_assert!(!divisor.is_zero());

        // Defer openness to the consumer (per-token flags); only freshness (below)
        // can still force closed. Some v11 feeds are 24/7 and keep publishing
        // valid prices even while their (extended) status is Unknown or Closed, so
        // whether to trade then is a per-token policy. On v8 this is unobserved and
        // AllowClosed is generally not advised, but staleness backstops it either way.
        let mut is_open = true;

        let observations_timestamp = report.observations_timestamp;

        let last_update_diff_secs = if let Some(last_update_timestamp_ns) =
            report.last_update_timestamp()
        {
            let observations_timestamp_ns = u64::from(observations_timestamp)
                .checked_mul(NANOS_PER_SECOND)
                .ok_or(crate::Error::Overflow(
                    "observations_timestamp is too large",
                ))?;
            let last_update_diff =
                match observations_timestamp_ns.checked_sub(last_update_timestamp_ns) {
                    Some(last_update_diff) => last_update_diff,
                    None => {
                        let abs_diff = observations_timestamp_ns.abs_diff(last_update_timestamp_ns);
                        // NOTE: Last update timestamp may exceed observations timestamp by <1s
                        // to avoid round‑down issues.
                        if abs_diff >= NANOS_PER_SECOND {
                            return Err(crate::Error::InvalidRange(
                                "last_update_timestamp > observations_timestamp by 1s or more",
                            ));
                        }
                        0
                    }
                };
            let diff = match u32::try_from(last_update_diff.div_ceil(NANOS_PER_SECOND)) {
                Ok(diff) => diff,
                Err(_) => {
                    // If `last_update_diff_secs` exceeds the range representable by a `u32`,
                    // we consider the data too old. According to Chainlink Data Streams'
                    // specification for `last_update_timestamp`, such a cause should be
                    // treated as the market being closed.
                    is_open = false;
                    u32::MAX
                }
            };
            Some(diff)
        } else {
            None
        };

        let mut price = Self::new(
            Report::DECIMALS - divisor_decimals,
            i64::from(observations_timestamp),
            (price / divisor).try_into().unwrap(),
            (bid / divisor).try_into().unwrap(),
            (ask / divisor).try_into().unwrap(),
            last_update_diff_secs.unwrap_or(0),
        );

        price.set_flag(PriceFlag::Open, is_open);

        if last_update_diff_secs.is_some() {
            price.set_flag(PriceFlag::LastUpdateDiffEnabled, true);
            price.set_flag(PriceFlag::LastUpdateDiffSecs, true);
        }

        // Persisting the status here is what makes the `is_open` change
        // migration-free. Base openness only flips closed -> open for prices this
        // code writes, and any report that was previously coarse-closed gets a
        // non-Disabled status here. So an old account (Open=false, status
        // Disabled) resolves closed via the base flag, and a new one (Open=true,
        // status Closed) resolves closed via the status — either version of a
        // stored price yields the same verdict at read time.
        price.set_market_status(canonical_market_status(report));

        Ok(price)
    }
}

impl From<ExtendedMarketStatus> for FeedMarketStatus {
    fn from(value: ExtendedMarketStatus) -> Self {
        match value {
            ExtendedMarketStatus::Unknown => Self::Unknown,
            ExtendedMarketStatus::PreMarket => Self::PreMarket,
            ExtendedMarketStatus::RegularHours => Self::RegularHours,
            ExtendedMarketStatus::PostMarket => Self::PostMarket,
            ExtendedMarketStatus::Overnight => Self::Overnight,
            ExtendedMarketStatus::Closed => Self::Closed,
        }
    }
}

fn canonical_market_status(report: &Report) -> FeedMarketStatus {
    canonical(report.market_status(), report.extended_market_status())
}

/// Maps a decoded report to the canonical [`FeedMarketStatus`].
///
/// A granular extended status is authoritative. Otherwise the decoder tells us
/// whether the report has a coarse market status at all: `Some` is mapped, and
/// `None` (no market-status concept) persists nothing.
fn canonical(
    coarse: Option<MarketStatus>,
    extended: Option<ExtendedMarketStatus>,
) -> FeedMarketStatus {
    if let Some(extended) = extended {
        return extended.into();
    }
    match coarse {
        Some(MarketStatus::Open) => FeedMarketStatus::RegularHours,
        Some(MarketStatus::Closed) => FeedMarketStatus::Closed,
        Some(MarketStatus::Unknown) => FeedMarketStatus::Unknown,
        None => FeedMarketStatus::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{ExtendedMarketStatus, MarketStatus};
    use gmsol_utils::price::market_status::MarketStatus as FeedMarketStatus;

    use crate::FromChainlinkReport;
    use gmsol_utils::price::feed_price::PriceFeedPrice;
    use gmsol_utils::price::market_status::{MarketStatusFlag, MarketStatusFlagContainer};

    fn v11_report(market_status: u32) -> Report {
        use chainlink_data_streams_report::report::v11::ReportDataV11;
        use num_bigint::BigInt;

        let mut feed_id_bytes = [0u8; 32];
        feed_id_bytes[1] = 0x0b; // version 11
        let feed_id = chainlink_data_streams_report::feed_id::ID(feed_id_bytes);
        let m: BigInt = "1000000000000000000".parse().unwrap();
        let data = ReportDataV11 {
            feed_id,
            valid_from_timestamp: 1000,
            observations_timestamp: 1000,
            native_fee: BigInt::from(100),
            link_fee: BigInt::from(200),
            expires_at: 1100,
            mid: BigInt::from(50000) * &m,
            last_seen_timestamp_ns: 1_000_000_000_000,
            bid: BigInt::from(49900) * &m,
            bid_volume: BigInt::from(1000) * &m,
            ask: BigInt::from(50100) * &m,
            ask_volume: BigInt::from(2000) * &m,
            last_traded_price: BigInt::from(50050) * &m,
            market_status,
        };
        crate::report::decode(&data.abi_encode().unwrap()).unwrap()
    }

    #[test]
    fn stores_status_and_defers() {
        // RegularHours: stored, open via default flags.
        let open = v11_report(2);
        let price = PriceFeedPrice::from_chainlink_report(&open).unwrap();
        assert!(price.market_status() == FeedMarketStatus::RegularHours);
        assert!(price.is_market_open(1000, u32::MAX, MarketStatusFlagContainer::default()));

        // Closed (=5): stored, closed via default flags.
        let closed = v11_report(5);
        let price = PriceFeedPrice::from_chainlink_report(&closed).unwrap();
        assert!(price.market_status() == FeedMarketStatus::Closed);
        assert!(!price.is_market_open(1000, u32::MAX, MarketStatusFlagContainer::default()));

        // RegularHours but the token halts it.
        let mut halt = MarketStatusFlagContainer::default();
        halt.set_flag(MarketStatusFlag::HaltRegularHours, true);
        let price = PriceFeedPrice::from_chainlink_report(&open).unwrap();
        assert!(!price.is_market_open(1000, u32::MAX, halt));
    }

    #[test]
    fn from_extended_maps_each_variant() {
        assert!(FeedMarketStatus::from(ExtendedMarketStatus::Unknown) == FeedMarketStatus::Unknown);
        assert!(
            FeedMarketStatus::from(ExtendedMarketStatus::PreMarket) == FeedMarketStatus::PreMarket
        );
        assert!(
            FeedMarketStatus::from(ExtendedMarketStatus::RegularHours)
                == FeedMarketStatus::RegularHours
        );
        assert!(
            FeedMarketStatus::from(ExtendedMarketStatus::PostMarket)
                == FeedMarketStatus::PostMarket
        );
        assert!(
            FeedMarketStatus::from(ExtendedMarketStatus::Overnight) == FeedMarketStatus::Overnight
        );
        assert!(FeedMarketStatus::from(ExtendedMarketStatus::Closed) == FeedMarketStatus::Closed);
    }

    #[test]
    fn canonical_resolves_coarse_and_extended() {
        // Granular extended status wins.
        assert!(
            canonical(
                Some(MarketStatus::Open),
                Some(ExtendedMarketStatus::PreMarket)
            ) == FeedMarketStatus::PreMarket
        );
        // Coarse status maps.
        assert!(canonical(Some(MarketStatus::Open), None) == FeedMarketStatus::RegularHours);
        assert!(canonical(Some(MarketStatus::Closed), None) == FeedMarketStatus::Closed);
        assert!(canonical(Some(MarketStatus::Unknown), None) == FeedMarketStatus::Unknown);
        // No market-status concept.
        assert!(canonical(None, None) == FeedMarketStatus::Disabled);
    }

    fn v8_report(market_status: u32) -> Report {
        use chainlink_data_streams_report::report::v8::ReportDataV8;
        use num_bigint::BigInt;

        let mut feed_id_bytes = [0u8; 32];
        feed_id_bytes[1] = 0x08; // version 8
        let feed_id = chainlink_data_streams_report::feed_id::ID(feed_id_bytes);
        let m: BigInt = "1000000000000000000".parse().unwrap();
        let data = ReportDataV8 {
            feed_id,
            valid_from_timestamp: 1000,
            observations_timestamp: 1000,
            native_fee: BigInt::from(100),
            link_fee: BigInt::from(200),
            expires_at: 1100,
            last_update_timestamp: 1_000_000_000_000,
            mid_price: BigInt::from(50000) * &m,
            market_status,
        };
        crate::report::decode(&data.abi_encode().unwrap()).unwrap()
    }

    #[test]
    fn allow_closed_opens_a_reported_closed_market() {
        let closed = v8_report(1); // 1 -> Closed
        let price = PriceFeedPrice::from_chainlink_report(&closed).unwrap();
        assert!(price.market_status() == FeedMarketStatus::Closed);

        // Default flags: a reported Closed stays closed.
        assert!(!price.is_market_open(1000, u32::MAX, MarketStatusFlagContainer::default()));

        // AllowClosed opens it while the price is fresh...
        let mut allow_closed = MarketStatusFlagContainer::default();
        allow_closed.set_flag(MarketStatusFlag::AllowClosed, true);
        assert!(price.is_market_open(1000, u32::MAX, allow_closed));

        // ...but staleness closes it regardless of the flag.
        assert!(!price.is_market_open(i64::MAX, 0, allow_closed));
    }
}
