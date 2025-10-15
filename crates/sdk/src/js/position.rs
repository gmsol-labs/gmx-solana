use std::sync::Arc;

use gmsol_model::PositionState;
use gmsol_programs::{gmsol_store::accounts::Position, model::PositionModel};
use wasm_bindgen::prelude::*;

use crate::{
    js::events::JsTradeEvent,
    position::{status::PositionStatus, PositionCalculations},
    utils::zero_copy::{
        try_deserialize_zero_copy, try_deserialize_zero_copy_from_base64_with_options,
    },
};

use super::{market::JsMarketModel, price::Prices};

/// JS version of [`Position`].
#[wasm_bindgen(js_name = Position)]
#[derive(Clone)]
pub struct JsPosition {
    pub(crate) position: Arc<Position>,
}

#[wasm_bindgen(js_class = Position)]
impl JsPosition {
    /// Create from base64 encoded account data with options.
    pub fn decode_from_base64_with_options(
        data: &str,
        no_discriminator: Option<bool>,
    ) -> crate::Result<Self> {
        let position = try_deserialize_zero_copy_from_base64_with_options(
            data,
            no_discriminator.unwrap_or(false),
        )?;

        Ok(Self {
            position: Arc::new(position.0),
        })
    }

    /// Create from base64 encoded account data.
    pub fn decode_from_base64(data: &str) -> crate::Result<Self> {
        Self::decode_from_base64_with_options(data, None)
    }

    /// Create from account data.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        let position = try_deserialize_zero_copy(data)?;

        Ok(Self {
            position: Arc::new(position.0),
        })
    }

    /// Convert to a [`JsPositionModel`].
    pub fn to_model(&self, market: &JsMarketModel) -> crate::Result<JsPositionModel> {
        Ok(JsPositionModel {
            model: PositionModel::new(market.model.clone(), self.position.clone())?,
        })
    }

    /// Create a clone of this position.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

/// JS version of [`PositionModel`].
#[wasm_bindgen(js_name = PositionModel)]
#[derive(Clone)]
pub struct JsPositionModel {
    model: PositionModel,
}

#[wasm_bindgen(js_class = PositionModel)]
impl JsPositionModel {
    /// Get position status.
    pub fn status(&self, prices: Prices) -> crate::Result<PositionStatus> {
        let prices = prices.into();
        self.model.status(&prices)
    }

    /// Get position size.
    pub fn size(&self) -> u128 {
        *self.model.size_in_usd()
    }

    /// Get position size in tokens.
    pub fn size_in_tokens(&self) -> u128 {
        *self.model.size_in_tokens()
    }

    /// Get collateral amount.
    pub fn collateral_amount(&self) -> u128 {
        *self.model.collateral_amount()
    }

    /// Returns the inner [`JsPosition`].
    pub fn position(&self) -> JsPosition {
        JsPosition {
            position: self.model.position_arc().clone(),
        }
    }

    /// Update with trade event.
    pub fn update_with_trade_event(
        &mut self,
        event: &JsTradeEvent,
        force_update: Option<bool>,
    ) -> crate::Result<bool> {
        let updated = self
            .model
            .update(&event.event.after.into(), force_update.unwrap_or_default());

        Ok(updated)
    }

    /// Create a clone of this position model.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

impl From<PositionModel> for JsPositionModel {
    fn from(model: PositionModel) -> Self {
        Self { model }
    }
}
