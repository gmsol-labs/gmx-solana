use std::sync::Arc;

use gmsol_model::{LiquidityMarket, LiquidityMarketExt, PnlFactorKind};
use gmsol_programs::{
    gmsol_store::accounts::Market,
    model::{MarketModel, PositionOptions},
};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::position::JsPositionModel,
    market::{MarketCalculations, MarketStatus},
    serde::StringPubkey,
    utils::zero_copy::{
        try_deserialize_zero_copy, try_deserialize_zero_copy_from_base64_with_options,
    },
};

use super::price::Prices;

/// Wrapper of [`Market`].
#[wasm_bindgen(js_name = Market)]
#[derive(Clone)]
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

    /// Create a clone of this market.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
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

/// Params for calculating max sellable avlue.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MaxSellableValueParams {
    /// Prices.
    pub prices: Prices,
}

/// Wrapper of [`MarketModel`].
#[wasm_bindgen(js_name = MarketModel)]
#[derive(Clone)]
pub struct JsMarketModel {
    pub(super) model: MarketModel,
}

#[wasm_bindgen(js_class = MarketModel)]
impl JsMarketModel {
    /// Get market token price.
    pub fn market_token_price(&self, params: MarketTokenPriceParams) -> crate::Result<u128> {
        let mut market_model = self.model.clone();
        Ok(market_model.with_vis_disabled(|market| {
            market.market_token_price(&params.prices.into(), params.pnl_factor, params.maximize)
        })?)
    }

    /// Calculates max sellable value.
    pub fn max_sellable_value(&self, params: MaxSellableValueParams) -> crate::Result<u128> {
        self.model.max_sellable_value(&params.prices.into())
    }

    /// Get market status.
    pub fn status(&self, params: MarketStatusParams) -> crate::Result<MarketStatus> {
        let prices = params.prices.into();
        let mut market_model = self.model.clone();
        market_model.with_vis_disabled(|market| market.status(&prices))
    }

    /// Returns current supply.
    pub fn supply(&self) -> u128 {
        self.model.total_supply()
    }

    /// Create an empty position model.
    pub fn create_empty_position(
        &self,
        args: CreateEmptyPositionArgs,
    ) -> crate::Result<JsPositionModel> {
        let CreateEmptyPositionArgs {
            is_long,
            collateral_token,
            owner,
            created_at,
            generate_bump,
            store_program_id,
        } = args;

        let mut options = PositionOptions::default();

        if let Some(owner) = owner {
            options.owner = Some(*owner);
        }

        if let Some(created_at) = created_at {
            options.created_at = created_at;
        }

        if let Some(generate_bump) = generate_bump {
            options.generate_bump = generate_bump;
        }

        if let Some(program_id) = store_program_id {
            options.store_program_id = *program_id;
        }

        let mut market_model = self.model.clone();
        let position = market_model.with_vis_disabled(|market| {
            market
                .clone()
                .into_empty_position_opts(is_long, *collateral_token, options)
        })?;

        Ok(position.into())
    }

    /// Create a clone of this market model.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }

    /// Set order fee discount factor.
    #[wasm_bindgen(js_name = setOrderFeeDiscountFactor)]
    pub fn set_order_fee_discount_factor(&mut self, factor: u128) {
        self.model.set_order_fee_discount_factor(factor);
    }
}

impl From<MarketModel> for JsMarketModel {
    fn from(model: MarketModel) -> Self {
        Self { model }
    }
}

/// Parameters for creating empty position model.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateEmptyPositionArgs {
    /// Is long side.
    pub is_long: bool,
    /// Collateral token.
    pub collateral_token: StringPubkey,
    /// The owner of the position.
    ///
    /// If set to `None`, the `owner` will use the default pubkey.
    #[serde(default)]
    pub owner: Option<StringPubkey>,
    /// The timestamp of the position creation.
    #[serde(default)]
    pub created_at: Option<i64>,
    /// Whether to generate a bump seed.
    ///
    /// If set `false`, the `bump` will be fixed to `0`.
    #[serde(default)]
    pub generate_bump: Option<bool>,
    /// The store program ID used to generate the bump seed.
    #[serde(default)]
    pub store_program_id: Option<StringPubkey>,
}
