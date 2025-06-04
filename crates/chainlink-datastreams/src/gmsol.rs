use gmsol_utils::price::{feed_price::PriceFeedPrice, find_divisor_decimals, PriceFlag, TEN, U192};

use crate::Report;

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

        let mut price = Self::new(
            Report::DECIMALS - divisor_decimals,
            i64::from(report.observations_timestamp),
            (price / divisor).try_into().unwrap(),
            (bid / divisor).try_into().unwrap(),
            (ask / divisor).try_into().unwrap(),
        );

        price.set_flag(PriceFlag::Open, true);

        Ok(price)
    }
}
