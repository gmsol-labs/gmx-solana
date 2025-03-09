use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_utils::InitSpace;

use crate::{
    states::{Seed, TokenConfig},
    CoreError,
};

use super::PriceProviderKind;

const MAX_FLAGS: usize = 8;

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

        require_gte!(price.ts, self.price.ts, CoreError::InvalidArgument);
        require_gte!(
            current_ts.saturating_add_unsigned(max_future_excess),
            price.ts,
            CoreError::InvalidArgument
        );
        require_gte!(price.max_price, price.min_price, CoreError::InvalidArgument);
        require_gte!(price.max_price, price.price, CoreError::InvalidArgument);
        require_gte!(price.price, price.min_price, CoreError::InvalidArgument);

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
    ) -> Result<(u64, i64, gmsol_utils::Price)> {
        let provider = self.provider()?;
        require_eq!(token_config.expected_provider()?, provider);
        let feed_id = token_config.get_feed(&provider)?;

        require_keys_eq!(self.feed_id, feed_id, CoreError::InvalidPriceFeedAccount);
        require!(self.price.is_market_open(), CoreError::MarketNotOpen);

        let timestamp = self.price.ts;
        let current = clock.unix_timestamp;
        if current > timestamp && current - timestamp > token_config.heartbeat_duration().into() {
            return Err(CoreError::PriceFeedNotUpdated.into());
        }

        let price = self.try_to_price(token_config)?;

        Ok((self.last_published_at_slot(), timestamp, price))
    }

    fn try_to_price(&self, token_config: &TokenConfig) -> Result<gmsol_utils::Price> {
        self.price().try_to_price(token_config)
    }
}

/// Price Feed Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum PriceFlag {
    /// Is Market Opened.
    Open,
    // CHECK: should have no more than `MAX_FLAGS` of flags.
}

gmsol_utils::flags!(PriceFlag, MAX_FLAGS, u8);

/// Price structure for Price Feed.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PriceFeedPrice {
    decimals: u8,
    flags: PriceFlagContainer,
    padding: [u8; 6],
    ts: i64,
    price: u128,
    min_price: u128,
    max_price: u128,
}

impl PriceFeedPrice {
    /// Get ts.
    pub fn ts(&self) -> i64 {
        self.ts
    }

    /// Get min price.
    pub fn min_price(&self) -> &u128 {
        &self.min_price
    }

    /// Get max price.
    pub fn max_price(&self) -> &u128 {
        &self.max_price
    }

    /// Get price.
    pub fn price(&self) -> &u128 {
        &self.price
    }

    /// Is market open.
    pub fn is_market_open(&self) -> bool {
        self.flags.get_flag(PriceFlag::Open)
    }

    /// Try converting to [`Price`](gmsol_utils::Price).
    pub fn try_to_price(&self, token_config: &TokenConfig) -> Result<gmsol_utils::Price> {
        use gmsol_utils::price::{Decimal, Price};

        let token_decimals = token_config.token_decimals();
        let precision = token_config.precision();

        let min = Decimal::try_from_price(self.min_price, self.decimals, token_decimals, precision)
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))?;

        let max = Decimal::try_from_price(self.max_price, self.decimals, token_decimals, precision)
            .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))?;

        Ok(Price { min, max })
    }

    /// Create from chainlink price report.
    pub fn from_chainlink_report(
        report: &gmsol_chainlink_datastreams::report::Report,
    ) -> Result<Self> {
        use gmsol_chainlink_datastreams::report::Report;
        use gmsol_utils::price::{find_divisor_decimals, TEN, U192};

        let price = report
            .non_negative_price()
            .ok_or_else(|| error!(CoreError::NegativePriceIsNotSupported))?;
        let bid = report
            .non_negative_bid()
            .ok_or_else(|| error!(CoreError::NegativePriceIsNotSupported))?;
        let ask = report
            .non_negative_ask()
            .ok_or_else(|| error!(CoreError::NegativePriceIsNotSupported))?;

        require!(ask >= price, CoreError::InvalidPriceReport);
        require!(price >= bid, CoreError::InvalidPriceReport);

        let divisor_decimals = find_divisor_decimals(&ask);

        require_gte!(Report::DECIMALS, divisor_decimals, CoreError::PriceOverflow);

        let divisor = TEN.pow(U192::from(divisor_decimals));

        debug_assert!(!divisor.is_zero());

        let mut price = Self {
            decimals: Report::DECIMALS - divisor_decimals,
            flags: Default::default(),
            padding: [0; 6],
            ts: i64::from(report.observations_timestamp),
            price: (price / divisor).try_into().unwrap(),
            min_price: (bid / divisor).try_into().unwrap(),
            max_price: (ask / divisor).try_into().unwrap(),
        };

        price.flags.set_flag(PriceFlag::Open, true);

        Ok(price)
    }
}
