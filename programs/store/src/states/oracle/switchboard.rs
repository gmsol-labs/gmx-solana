use std::ops::Deref;

use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use anchor_lang::ZeroCopy;
use gmsol_utils::price::Decimal;
use gmsol_utils::price::Price;
use switchboard_on_demand::Discriminator as _;

use crate::{states::TokenConfig, CoreError};

mod id {
    use anchor_lang::prelude::Pubkey;
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(feature = "devnet")] {
            pub(super) const ID: Pubkey = switchboard_on_demand::ON_DEMAND_DEVNET_PID;
        } else {
            pub(super) const ID: Pubkey = switchboard_on_demand::ON_DEMAND_MAINNET_PID;
        }
    }
}

/// The Switchboard receiver program.
pub struct Switchboard;

impl Id for Switchboard {
    fn id() -> Pubkey {
        id::ID
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
    const DISCRIMINATOR: &'static [u8] = &switchboard_on_demand::SbFeed::DISCRIMINATOR;
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
        let result_ts = feed.result_ts();
        require_gte!(
            result_ts.saturating_add(token_config.heartbeat_duration().into()),
            clock.unix_timestamp,
            CoreError::PriceFeedNotUpdated
        );
        Ok((
            feed.result_land_slot(),
            result_ts,
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
