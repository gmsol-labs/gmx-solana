use std::sync::Arc;

use gmsol_programs::gmsol_store::events::TradeEvent;
use wasm_bindgen::prelude::*;

use crate::{
    js::{
        market::{CreateEmptyPositionArgs, JsMarketModel},
        position::JsPositionModel,
    },
    utils::{base64::decode_base64, events::decode_anchor_event_with_options},
};

/// JS version of [`TradeEvent`].
#[wasm_bindgen(js_name = TradeEvent)]
pub struct JsTradeEvent {
    pub(crate) event: Arc<TradeEvent>,
}

#[wasm_bindgen(js_class = TradeEvent)]
impl JsTradeEvent {
    /// Create from base64 encoded event data with options.
    pub fn decode_from_base64_with_options(
        data: &str,
        no_discriminator: Option<bool>,
    ) -> crate::Result<Self> {
        let data = decode_base64(data)?;
        Self::decode_with_options(&data, no_discriminator)
    }

    /// Create from base64 encoded event data.
    pub fn decode_from_base64(data: &str) -> crate::Result<Self> {
        Self::decode_from_base64_with_options(data, None)
    }

    /// Create from event data.
    pub fn decode_with_options(data: &[u8], no_discriminator: Option<bool>) -> crate::Result<Self> {
        let event = decode_anchor_event_with_options(data, no_discriminator.unwrap_or_default())?;
        Ok(Self {
            event: Arc::new(event),
        })
    }

    /// Convert into a position model.
    pub fn to_position_model(&self, market: &JsMarketModel) -> crate::Result<JsPositionModel> {
        if self.event.market_token != market.model.meta.market_token_mint {
            return Err(crate::Error::custom(
                "invalid argument: market token mint does not match",
            ));
        }

        let is_long = self.event.is_long();
        let collateral_token = if self.event.is_collateral_long() {
            market.model.meta.long_token_mint
        } else {
            market.model.meta.short_token_mint
        };

        let mut position = market.create_empty_position(CreateEmptyPositionArgs {
            is_long,
            collateral_token: collateral_token.into(),
            owner: Some(self.event.user.into()),
            created_at: None,
            generate_bump: None,
            store_program_id: None,
        })?;

        position.update_with_trade_event(self, None)?;

        Ok(position)
    }
}
