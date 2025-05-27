use crate::core::{market::MarketMeta, token_config::TokenMapAccess};

/// Token decimals for a market.
#[derive(Debug, Clone, Copy)]
pub struct MarketDecimals {
    /// Index token decimals.
    pub index_token_decimals: u8,
    /// Long token decimals.
    pub long_token_decimals: u8,
    /// Short token decimals.
    pub short_token_decimals: u8,
}

impl MarketDecimals {
    /// Create from market meta and token map.
    pub fn new(meta: &MarketMeta, token_map: &impl TokenMapAccess) -> crate::Result<Self> {
        let index_token_decimals = token_map
            .get(&meta.index_token_mint)
            .ok_or_else(|| crate::Error::NotFound)?
            .token_decimals;
        let long_token_decimals = token_map
            .get(&meta.long_token_mint)
            .ok_or_else(|| crate::Error::NotFound)?
            .token_decimals;
        let short_token_decimals = token_map
            .get(&meta.short_token_mint)
            .ok_or_else(|| crate::Error::NotFound)?
            .token_decimals;

        Ok(Self {
            index_token_decimals,
            long_token_decimals,
            short_token_decimals,
        })
    }
}
