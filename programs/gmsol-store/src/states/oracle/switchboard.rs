use crate::{states::TokenConfig, CoreError};
use anchor_lang::prelude::*;
use gmsol_utils::price::Decimal;
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
        let max_age = clock.unix_timestamp - token_config.heartbeat_duration() as i64;
        let (min_result_ts, _) = feed.current_result_ts_range();
        if min_result_ts < max_age {
            return Err(error!(CoreError::PriceIsStale));
        }
        Ok((
            feed.result.slot,
            feed.result_ts(),
            Self::price_from(&feed, token_config)?,
        ))
    }

    fn price_from(feed: &SbFeed, token_config: &TokenConfig) -> Result<Price> {
        let min_price = feed
            .min_value()
            .ok_or_else(|| error!(CoreError::PriceIsStale))?;
        let min_price = Decimal::try_from_price(
            min_price.mantissa() as u128,
            min_price.scale() as u8,
            token_config.token_decimals(),
            token_config.precision(),
        )
        .map_err(|_| error!(CoreError::PriceIsStale))?;
        let max_price = feed
            .max_value()
            .ok_or_else(|| error!(CoreError::PriceIsStale))?;
        let max_price = Decimal::try_from_price(
            max_price.mantissa() as u128,
            max_price.scale() as u8,
            token_config.token_decimals(),
            token_config.precision(),
        )
        .map_err(|_| error!(CoreError::PriceIsStale))?;
        Ok(Price {
            min: min_price,
            max: max_price,
        })
    }
}
