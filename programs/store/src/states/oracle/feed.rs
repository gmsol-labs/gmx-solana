use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_utils::{price::Decimal, InitSpace};

use crate::{
    states::{Seed, TokenConfig},
    CoreError,
};

use super::{OraclePriceParts, PriceProviderKind};

pub use gmsol_utils::price::feed_price::PriceFeedPrice;

/// Custom Price Feed.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PriceFeed {
    pub(crate) bump: u8,
    pub(crate) provider: u8,
    pub(crate) index: u16,
    padding_0: [u8; 12],
    pub(crate) store: Pubkey,
    /// Authority.
    pub authority: Pubkey,
    pub(crate) token: Pubkey,
    pub(crate) feed_id: Pubkey,
    last_published_at_slot: u64,
    last_published_at: i64,
    price: PriceFeedPrice,
    reserved: [u8; 256],
}

impl InitSpace for PriceFeed {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for PriceFeed {
    const SEED: &'static [u8] = b"price_feed";
}

impl Default for PriceFeed {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl PriceFeed {
    pub(crate) fn init(
        &mut self,
        bump: u8,
        index: u16,
        provider: PriceProviderKind,
        store: &Pubkey,
        authority: &Pubkey,
        token: &Pubkey,
        feed_id: &Pubkey,
    ) -> Result<()> {
        self.bump = bump;
        self.provider = provider.into();
        self.index = index;
        self.store = *store;
        self.authority = *authority;
        self.token = *token;
        self.feed_id = *feed_id;
        Ok(())
    }

    /// Returns whether the price is updated.
    pub(crate) fn update(
        &mut self,
        price: &PriceFeedPrice,
        max_future_excess: u64,
        idempotent: bool,
    ) -> Result<bool> {
        let clock = Clock::get()?;
        let slot = clock.slot;
        let current_ts = clock.unix_timestamp;

        // Validate time.
        require_gte!(
            slot,
            self.last_published_at_slot,
            CoreError::PreconditionsAreNotMet
        );
        require_gte!(
            current_ts,
            self.last_published_at,
            CoreError::PreconditionsAreNotMet
        );

        // If idempotent updates are allowed, skip older price updates.
        let price_ts = price.ts();
        let last_price_ts = self.price.ts();
        if idempotent && price_ts < last_price_ts {
            return Ok(false);
        }

        require_gte!(price_ts, last_price_ts, CoreError::InvalidArgument);
        require_gte!(
            current_ts.saturating_add_unsigned(max_future_excess),
            price.ts(),
            CoreError::InvalidArgument
        );
        require_gte!(
            *price.max_price(),
            *price.min_price(),
            CoreError::InvalidArgument
        );
        require_gte!(
            *price.max_price(),
            *price.price(),
            CoreError::InvalidArgument
        );
        require_gte!(
            *price.price(),
            *price.min_price(),
            CoreError::InvalidArgument
        );

        self.last_published_at_slot = slot;
        self.last_published_at = current_ts;
        self.price = *price;

        Ok(true)
    }

    /// Get provider.
    pub fn provider(&self) -> Result<PriceProviderKind> {
        PriceProviderKind::try_from(self.provider)
            .map_err(|_| error!(CoreError::InvalidProviderKindIndex))
    }

    /// Get price feed price.
    pub fn price(&self) -> &PriceFeedPrice {
        &self.price
    }

    /// Get published slot.
    pub fn last_published_at_slot(&self) -> u64 {
        self.last_published_at_slot
    }

    /// Get feed id.
    pub fn feed_id(&self) -> &Pubkey {
        &self.feed_id
    }

    pub(crate) fn check_and_get_price(
        &self,
        clock: &Clock,
        token_config: &TokenConfig,
        allow_closed: bool,
    ) -> Result<OraclePriceParts> {
        let provider = self.provider()?;
        require_eq!(
            token_config.expected_provider().map_err(CoreError::from)?,
            provider
        );

        let feed_config = token_config
            .get_feed_config(&provider)
            .map_err(CoreError::from)?;
        require_keys_eq!(
            self.feed_id,
            *feed_config.feed(),
            CoreError::InvalidPriceFeedAccount
        );

        let current = clock.unix_timestamp;
        let heartbeat_duration = token_config.heartbeat_duration();
        let is_open = self.price.is_market_open(
            current,
            heartbeat_duration,
            feed_config.market_status_flags(),
        );

        if !allow_closed {
            require!(is_open, CoreError::MarketNotOpen);
        }

        let timestamp = self.price.ts();
        if current > timestamp && current - timestamp > heartbeat_duration.into() {
            return Err(CoreError::PriceFeedNotUpdated.into());
        }

        let price = self.try_to_price(token_config)?;
        let ref_price = self.try_to_ref_price(token_config)?;

        Ok(OraclePriceParts {
            oracle_slot: self.last_published_at_slot(),
            oracle_ts: timestamp,
            price,
            ref_price: Some(ref_price),
            is_open,
        })
    }

    fn try_to_price(&self, token_config: &TokenConfig) -> Result<gmsol_utils::Price> {
        self.price()
            .try_to_price(token_config)
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))
    }

    fn try_to_ref_price(&self, token_config: &TokenConfig) -> Result<Decimal> {
        self.price()
            .try_to_ref_price(token_config)
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmsol_utils::token_config::{FeedConfig, UpdateTokenConfigParams};

    #[test]
    fn default_heartbeat_accepts_thirty_seconds_and_rejects_thirty_one() {
        let provider = PriceProviderKind::ChainlinkDataStreams;
        let feed_id = Pubkey::new_unique();
        let mut feed = PriceFeed::default();
        feed.init(
            0,
            0,
            provider,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &feed_id,
        )
        .expect("feed initializes");
        feed.price = PriceFeedPrice::new(0, 100, 100, 100, 100, 0);

        let mut config = TokenConfig::zeroed();
        config
            .set_feed_config(&provider, FeedConfig::new(feed_id))
            .expect("feed config is valid");
        config.set_expected_provider(provider);
        config.heartbeat_duration = UpdateTokenConfigParams::default().heartbeat_duration;
        assert_eq!(config.heartbeat_duration, 30);

        for age in [29, 30] {
            let clock = Clock {
                unix_timestamp: 100 + age,
                ..Clock::default()
            };
            assert!(feed.check_and_get_price(&clock, &config, true).is_ok());
        }
        let stale_clock = Clock {
            unix_timestamp: 131,
            ..Clock::default()
        };
        assert!(feed
            .check_and_get_price(&stale_clock, &config, true)
            .is_err());
    }
}
