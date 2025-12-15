use gmsol_model::price::{Price, Prices};
use gmsol_programs::{gmsol_store::types::MarketMeta, model::MarketModel};
use solana_sdk::pubkey::Pubkey;

/// Performs Market calculations.
pub trait MarketCalculator {
    /// Returns [`Prices`] corresponding to the given [`MarketMeta`].
    fn get_token_prices_for_market_meta(&self, meta: &MarketMeta) -> Option<Prices<u128>> {
        let index_token_price = self.get_token_price(&meta.index_token_mint)?;
        let long_token_price = self.get_token_price(&meta.long_token_mint)?;
        let short_token_price = self.get_token_price(&meta.short_token_mint)?;
        Some(Prices {
            index_token_price,
            long_token_price,
            short_token_price,
        })
    }

    /// Returns [`MarketModel`] and [`Prices`] corresponding to the given market token.
    fn get_market_model_with_prices(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<(&MarketModel, Prices<u128>)> {
        let market = self
            .get_market_model(market_token)
            .ok_or_else(|| crate::Error::NotFound)?;
        let meta = &market.meta;
        let prices = self
            .get_token_prices_for_market_meta(meta)
            .ok_or_else(|| crate::Error::NotFound)?;
        Ok((market, prices))
    }

    /// Returns [`MarketModel`] corresponding to the given market token.
    fn get_market_model(&self, market_token: &Pubkey) -> Option<&MarketModel>;

    /// Returns [`Price`] corresponding to the given token address.
    fn get_token_price(&self, token: &Pubkey) -> Option<Price<u128>>;
}
