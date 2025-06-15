use anchor_lang::prelude::zero_copy;

use crate::{
    price::{
        decimal::{Decimal, DecimalError},
        Price, PriceFlag, MAX_PRICE_FLAG,
    },
    token_config::TokenConfig,
};

crate::flags!(PriceFlag, MAX_PRICE_FLAG, u8);

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
    /// Create a new [`PriceFeedPrice`].
    pub fn new(decimals: u8, ts: i64, price: u128, min_price: u128, max_price: u128) -> Self {
        Self {
            decimals,
            flags: Default::default(),
            padding: [0; 6],
            ts,
            price,
            min_price,
            max_price,
        }
    }

    /// Set flag.
    pub fn set_flag(&mut self, flag: PriceFlag, value: bool) -> bool {
        self.flags.set_flag(flag, value)
    }

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

    /// Try converting to [`Price`].
    pub fn try_to_price(&self, token_config: &TokenConfig) -> Result<Price, DecimalError> {
        let token_decimals = token_config.token_decimals();
        let precision = token_config.precision();

        let min =
            Decimal::try_from_price(self.min_price, self.decimals, token_decimals, precision)?;

        let max =
            Decimal::try_from_price(self.max_price, self.decimals, token_decimals, precision)?;

        Ok(Price { min, max })
    }

    /// Returns reference price in [`Decimal`].
    pub fn try_to_ref_price(&self, token_config: &TokenConfig) -> Result<Decimal, DecimalError> {
        let token_decimals = token_config.token_decimals();
        let precision = token_config.precision();
        Decimal::try_from_price(self.price, self.decimals, token_decimals, precision)
    }
}
