use std::ops::Deref;

use crate::{states::TokenConfig, CoreError};
use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use anchor_lang::ZeroCopy;
use gmsol_utils::price::Decimal;
use gmsol_utils::price::Price;
use switchboard_on_demand::Discriminator as _;

/// The Switchboard receiver program.
pub struct Switchboard;

#[cfg(feature = "devnet")]
impl Id for Switchboard {
    fn id() -> Pubkey {
        switchboard_on_demand::ON_DEMAND_DEVNET_PID
    }
}

#[cfg(not(feature = "devnet"))]
impl Id for Switchboard {
    fn id() -> Pubkey {
        switchboard_on_demand::ON_DEMAND_MAINNET_PID
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SbFeed {
    feed: switchboard_on_demand::SbFeed,
}

impl Deref for SbFeed {
    type Target = switchboard_on_demand::SbFeed;

    fn deref(&self) -> &Self::Target {
        &self.feed
    }
}

impl ZeroCopy for SbFeed {}

impl Owner for SbFeed {
    fn owner() -> Pubkey {
        Switchboard::id()
    }
}

impl Discriminator for SbFeed {
    const DISCRIMINATOR: [u8; 8] = switchboard_on_demand::SbFeed::DISCRIMINATOR;
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
        let (min_result_ts, _) = feed.current_result_ts_range();
        require_gte!(
            min_result_ts.saturating_add(token_config.heartbeat_duration().into()),
            clock.unix_timestamp,
            CoreError::PriceFeedNotUpdated
        );
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
        let min_price = from_rust_decimal_price(&min_price, token_config)?;
        let max_price = feed
            .max_value()
            .ok_or_else(|| error!(CoreError::PriceIsStale))?;
        let max_price = from_rust_decimal_price(&max_price, token_config)?;
        Ok(Price {
            min: min_price,
            max: max_price,
        })
    }
}

fn from_rust_decimal_price(
    price: &rust_decimal::Decimal,
    token_config: &TokenConfig,
) -> Result<Decimal> {
    Decimal::try_from_price(
        price
            .mantissa()
            .try_into()
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))?,
        price
            .scale()
            .try_into()
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))?,
        token_config.token_decimals(),
        token_config.precision(),
    )
    .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))
}
