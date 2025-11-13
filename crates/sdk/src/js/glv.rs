use std::sync::Arc;

use gmsol_programs::gmsol_store::accounts::Glv;
use wasm_bindgen::prelude::*;

use crate::{
    glv::GlvModel,
    utils::zero_copy::{
        try_deserialize_zero_copy_from_base64_with_options, try_deserialize_zero_copy_with_options,
    },
};

/// Wrapper of [`Glv`].
#[wasm_bindgen(js_name = Glv)]
#[derive(Clone)]
pub struct JsGlv {
    glv: Arc<Glv>,
}

#[wasm_bindgen(js_class = Glv)]
impl JsGlv {
    /// Create from base64 encoded account data with options.
    pub fn decode_from_base64_with_options(
        data: &str,
        no_discriminator: Option<bool>,
    ) -> crate::Result<Self> {
        let glv = try_deserialize_zero_copy_from_base64_with_options(
            data,
            no_discriminator.unwrap_or(false),
        )?;

        Ok(Self {
            glv: Arc::new(glv.0),
        })
    }

    /// Create from base64 encoded account data with options.
    pub fn decode_with_options(data: &[u8], no_discriminator: Option<bool>) -> crate::Result<Self> {
        let glv = try_deserialize_zero_copy_with_options(data, no_discriminator.unwrap_or(false))?;

        Ok(Self {
            glv: Arc::new(glv.0),
        })
    }

    /// Convert into [`JsGlvModel`].
    pub fn to_model(&self, supply: u128) -> crate::Result<JsGlvModel> {
        Ok(JsGlvModel {
            model: GlvModel::new(self.glv.clone(), supply.try_into()?),
        })
    }

    /// Returns GLV token address.
    pub fn glv_token_address(&self) -> String {
        self.glv.glv_token.to_string()
    }

    /// Returns long token address.
    pub fn long_token_address(&self) -> String {
        self.glv.long_token.to_string()
    }

    /// Returns short token address.
    pub fn short_token_address(&self) -> String {
        self.glv.short_token.to_string()
    }

    /// Create a clone of this market.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

/// Wrapper of [`GlvModel`].
#[wasm_bindgen(js_name = GlvModel)]
#[derive(Clone)]
pub struct JsGlvModel {
    pub(super) model: GlvModel,
}

#[wasm_bindgen(js_class = GlvModel)]
impl JsGlvModel {
    /// Returns current supply.
    pub fn supply(&self) -> u128 {
        self.model.supply().into()
    }

    /// Returns GLV token address.
    pub fn glv_token_address(&self) -> String {
        self.model.glv_token.to_string()
    }

    /// Returns long token address.
    pub fn long_token_address(&self) -> String {
        self.model.long_token.to_string()
    }

    /// Returns short token address.
    pub fn short_token_address(&self) -> String {
        self.model.short_token.to_string()
    }

    /// Create a clone of this market model.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

impl From<GlvModel> for JsGlvModel {
    fn from(model: GlvModel) -> Self {
        Self { model }
    }
}
