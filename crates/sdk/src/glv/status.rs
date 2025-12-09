use crate::market::Value;

/// GLV Status.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct GlvStatus {
    /// The estimated max sellable value in the GLV.
    pub max_sellable_value: u128,
    /// The estimated total GLV value.
    pub total_value: Value,
}
