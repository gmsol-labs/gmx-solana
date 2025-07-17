use gmsol_utils::price::{feed_price::PriceFeedPrice, find_divisor_decimals, PriceFlag, TEN, U192};

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

        let mut is_open = match report.market_status() {
            MarketStatus::Unknown => {
                return Err(crate::Error::UnknownMarketStatus);
            }
            MarketStatus::Closed => false,
            MarketStatus::Open => true,
        };

        let observations_timestamp = report.observations_timestamp;

        let last_update_diff_ns =
            if let Some(last_update_timestamp_ns) = report.last_update_timestamp() {
                let observations_timestamp_ns = u64::from(observations_timestamp)
                    .checked_mul(NANOS_PER_SECOND)
                    .ok_or(crate::Error::Overflow(
                        "observations_timestamp is too large",
                    ))?;
                let last_update_diff = observations_timestamp_ns
                    .checked_sub(last_update_timestamp_ns)
                    .ok_or(crate::Error::InvalidRange(
                        "last_update_timestamp larger than observations_timestamp",
                    ))?;
                match u32::try_from(last_update_diff) {
                    Ok(diff) => diff,
                    Err(_) => {
                        // If `last_update_diff_ns` exceeds the range representable by a `u32`,
                        // we consider the data too old. According to Chainlink Data Streams'
                        // specification for `last_update_timestamp`, such a cause should be
                        // treasted as the market being closed.
                        is_open = false;
                        u32::MAX
                    }
                }
            } else {
                0
            };

        let mut price = Self::new(
            Report::DECIMALS - divisor_decimals,
            i64::from(observations_timestamp),
            (price / divisor).try_into().unwrap(),
            (bid / divisor).try_into().unwrap(),
            (ask / divisor).try_into().unwrap(),
            last_update_diff_ns,
        );

        price.set_flag(PriceFlag::Open, is_open);

        Ok(price)
    }
}
