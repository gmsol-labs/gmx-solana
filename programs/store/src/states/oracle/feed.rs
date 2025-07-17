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

    pub(crate) fn update(&mut self, price: &PriceFeedPrice, max_future_excess: u64) -> Result<()> {
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

        require_gte!(price.ts(), self.price.ts(), CoreError::InvalidArgument);
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

        Ok(())
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
    ) -> Result<OraclePriceParts> {
        let provider = self.provider()?;
        require_eq!(
            token_config.expected_provider().map_err(CoreError::from)?,
            provider
        );

        let feed_id = token_config.get_feed(&provider).map_err(CoreError::from)?;
        require_keys_eq!(self.feed_id, feed_id, CoreError::InvalidPriceFeedAccount);

        let current = clock.unix_timestamp;
        let heartbeat_duration = token_config.heartbeat_duration();
        require!(
            self.price.is_market_open(current, heartbeat_duration),
            CoreError::MarketNotOpen
        );

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
