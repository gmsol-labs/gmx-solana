use crate::{states::TokenConfig, CoreError};
use anchor_lang::prelude::*;
use gmsol_utils::price::Price;
use switchboard_on_demand::{SbFeed, ON_DEMAND_MAINNET_PID};

/// The Switchboard receiver program.
pub struct Switchboard;

impl Id for Switchboard {
    fn id() -> Pubkey {
        ON_DEMAND_MAINNET_PID
    }
}

impl Switchboard {
    #[allow(clippy::manual_inspect)]
    pub(super) fn check_and_get_price<'info>(
        clock: &Clock,
        token_config: &TokenConfig,
        feed: &'info AccountInfo<'info>,
    ) -> Result<(u64, i64, Price)> {
        let feed = AccountLoader::<SbFeed>::try_from(feed)?;
        let feed = feed.load()?;
        let max_age: u64 = token_config.heartbeat_duration().into();
        let oldest_slot = clock.slot - max_age;
        // TODO: heartbeat_duration is supposed to be in seconds, not slots
        // Review again in PR review there are other options
        if feed.result.min_slot().unwrap_or(0) < oldest_slot {
            return Err(error!(CoreError::PriceIsStale));
        }
        Ok((feed.result.slot, feed.result_ts(), Self::price_from(&feed)?))
    }

    fn price_from(feed: &SbFeed) -> Result<Price> {
        let min_price = feed.min_value().try_into();
        let max_price = feed.max_value().try_into();
        if let (Ok(min_price), Ok(max_price)) = (min_price, max_price) {
            return Ok(Price { min: min_price, max: max_price });
        }
        Err(error!(CoreError::InvalidPriceFeedPrice))
    }
}


