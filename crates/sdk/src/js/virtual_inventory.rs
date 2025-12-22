use std::sync::Arc;

use gmsol_programs::{gmsol_store::accounts::VirtualInventory, model::VirtualInventoryModel};
use wasm_bindgen::prelude::*;

use crate::utils::zero_copy::{
    try_deserialize_zero_copy, try_deserialize_zero_copy_from_base64_with_options,
};

/// Wrapper of [`VirtualInventory`].
#[wasm_bindgen(js_name = VirtualInventory)]
#[derive(Clone)]
pub struct JsVirtualInventory {
    virtual_inventory: Arc<VirtualInventory>,
}

#[wasm_bindgen(js_class = VirtualInventory)]
impl JsVirtualInventory {
    /// Create from base64 encoded account data with options.
    pub fn decode_from_base64_with_options(
        data: &str,
        no_discriminator: Option<bool>,
    ) -> crate::Result<Self> {
        let virtual_inventory = try_deserialize_zero_copy_from_base64_with_options(
            data,
            no_discriminator.unwrap_or(false),
        )?;

        Ok(Self {
            virtual_inventory: Arc::new(virtual_inventory.0),
        })
    }

    /// Create from base64 encoded account data.
    pub fn decode_from_base64(data: &str) -> crate::Result<Self> {
        Self::decode_from_base64_with_options(data, None)
    }

    /// Create from account data.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        let virtual_inventory = try_deserialize_zero_copy(data)?;

        Ok(Self {
            virtual_inventory: Arc::new(virtual_inventory.0),
        })
    }

    /// Convert into [`JsVirtualInventoryModel`].
    pub fn to_model(&self) -> JsVirtualInventoryModel {
        JsVirtualInventoryModel {
            model: VirtualInventoryModel::from_parts(self.virtual_inventory.clone()),
        }
    }

    /// Create a clone of this virtual inventory.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

/// Wrapper of [`VirtualInventoryModel`].
#[wasm_bindgen(js_name = VirtualInventoryModel)]
#[derive(Clone)]
pub struct JsVirtualInventoryModel {
    pub(super) model: VirtualInventoryModel,
}

#[wasm_bindgen(js_class = VirtualInventoryModel)]
impl JsVirtualInventoryModel {
    /// Create a clone of this virtual inventory model.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

impl From<VirtualInventoryModel> for JsVirtualInventoryModel {
    fn from(model: VirtualInventoryModel) -> Self {
        Self { model }
    }
}
