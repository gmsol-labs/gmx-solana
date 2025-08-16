use std::sync::Arc;

use gmsol_model::{LiquidityMarketExt, PnlFactorKind};
use gmsol_programs::{gmsol_store::accounts::Market, model::MarketModel};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    market::{MarketCalculations, MarketStatus},
    utils::zero_copy::{
        try_deserialize_zero_copy, try_deserialize_zero_copy_from_base64_with_options,
    },
};

use super::price::Prices;

/// Wrapper of [`Market`].
#[wasm_bindgen(js_name = Market)]
pub struct JsMarket {
    market: Arc<Market>,
}

#[wasm_bindgen(js_class = Market)]
impl JsMarket {
    /// Create from base64 encoded account data with options.
    pub fn decode_from_base64_with_options(
        data: &str,
        no_discriminator: Option<bool>,
    ) -> crate::Result<Self> {
        let market = try_deserialize_zero_copy_from_base64_with_options(
            data,
            no_discriminator.unwrap_or(false),
        )?;

        Ok(Self {
            market: Arc::new(market.0),
        })
    }

    /// Create from base64 encoded account data.
    pub fn decode_from_base64(data: &str) -> crate::Result<Self> {
        Self::decode_from_base64_with_options(data, None)
    }

    /// Create from account data.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        let market = try_deserialize_zero_copy(data)?;

        Ok(Self {
            market: Arc::new(market.0),
        })
    }

    /// Convert into [`JsMarketModel`]
    pub fn to_model(&self, supply: u64) -> JsMarketModel {
        JsMarketModel {
            model: MarketModel::from_parts(self.market.clone(), supply),
        }
    }

    /// Get market token address.
    pub fn market_token_address(&self) -> String {
        self.market.meta.market_token_mint.to_string()
    }

    /// Get index token address.
    pub fn index_token_address(&self) -> String {
        self.market.meta.index_token_mint.to_string()
    }

    /// Get long token address.
    pub fn long_token_address(&self) -> String {
        self.market.meta.long_token_mint.to_string()
    }

    /// Get short token address.
    pub fn short_token_address(&self) -> String {
        self.market.meta.short_token_mint.to_string()
    }
}

/// Params for calculating market token price.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MarketTokenPriceParams {
    /// Prices.
    pub prices: Prices,
    /// Pnl Factor.
    #[serde(default = "default_pnl_factor")]
    pub pnl_factor: PnlFactorKind,
    /// Maximize.
    pub maximize: bool,
}

fn default_pnl_factor() -> PnlFactorKind {
    PnlFactorKind::MaxAfterDeposit
}

/// Params for calculating market status.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MarketStatusParams {
    /// Prices.
    pub prices: Prices,
}

/// Wrapper of [`MarketModel`].
#[wasm_bindgen(js_name = MarketModel)]
pub struct JsMarketModel {
    pub(super) model: MarketModel,
}

#[wasm_bindgen(js_class = MarketModel)]
impl JsMarketModel {
    /// Get market token price.
    pub fn market_token_price(&self, params: MarketTokenPriceParams) -> crate::Result<u128> {
        Ok(self.model.market_token_price(
            &params.prices.into(),
            params.pnl_factor,
            params.maximize,
        )?)
    }

    /// Get market status.
    pub fn status(&self, params: MarketStatusParams) -> crate::Result<MarketStatus> {
        let prices = params.prices.into();
        self.model.status(&prices)
    }
}

impl From<MarketModel> for JsMarketModel {
    fn from(model: MarketModel) -> Self {
        Self { model }
    }
}
