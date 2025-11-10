//! Order simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    js::simulation::{encode_borsh_base64, encode_bytemuck_base64},
    serde::StringPubkey,
    simulation::order::OrderSimulationOutput,
};

use crate::js::position::JsPositionModel;

/// Arguments for order simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateOrderArgs {
    pub(crate) kind: CreateOrderKind,
    pub(crate) params: CreateOrderParams,
    pub(crate) collateral_or_swap_out_token: StringPubkey,
    #[serde(default)]
    pub(crate) pay_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub(crate) prefer_swap_out_token_update: Option<bool>,
    #[serde(default)]
    pub(crate) skip_limit_price_validation: Option<bool>,
    #[serde(default)]
    pub(crate) limit_swap_slippage: Option<u128>,
    #[serde(default)]
    pub(crate) update_prices_for_limit_order: Option<bool>,
}

/// A JS binding for [`OrderSimulationOutput`].
#[wasm_bindgen(js_name = OrderSimulationOutput)]
pub struct JsOrderSimulationOutput {
    pub(crate) output: OrderSimulationOutput,
}

/// Simulation output for increase order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct IncreaseOrderSimulationOutput {
    swaps: Vec<String>,
    report: String,
    position: Option<String>,
}

/// Simulation output for decrease order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DecreaseOrderSimulationOutput {
    swaps: Vec<String>,
    report: String,
    position: Option<String>,
    decrease_swap: Option<String>,
}

/// Simulation output for swap order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SwapOrderSimulationOutput {
    output_token: StringPubkey,
    amount: u128,
    report: Vec<String>,
}

#[wasm_bindgen(js_class = OrderSimulationOutput)]
impl JsOrderSimulationOutput {
    /// Returns increase order simulation output.
    pub fn increase(
        &self,
        skip_position: Option<bool>,
    ) -> crate::Result<Option<IncreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Increase {
            swaps,
            report,
            position,
        } = &self.output
        {
            let encode_position = !skip_position.unwrap_or_default();
            Ok(Some(IncreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_position.then(|| encode_bytemuck_base64(position.position())),
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns decrease order simulation output.
    pub fn decrease(
        &self,
        skip_position: Option<bool>,
    ) -> crate::Result<Option<DecreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Decrease {
            swaps,
            report,
            position,
        } = &self.output
        {
            let encode_position = !skip_position.unwrap_or_default();
            Ok(Some(DecreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_position.then(|| encode_bytemuck_base64(position.position())),
                decrease_swap: position
                    .swap_history()
                    .first()
                    .map(|s| encode_borsh_base64(&**s))
                    .transpose()?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns swap order simulation output.
    pub fn swap(&self) -> crate::Result<Option<SwapOrderSimulationOutput>> {
        if let OrderSimulationOutput::Swap(swap) = &self.output {
            Ok(Some(SwapOrderSimulationOutput {
                output_token: (*swap.output_token()).into(),
                amount: swap.amount(),
                report: swap
                    .reports()
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns the result position model.
    pub fn position_model(&self) -> Option<JsPositionModel> {
        match &self.output {
            OrderSimulationOutput::Increase { position, .. }
            | OrderSimulationOutput::Decrease { position, .. } => {
                Some(JsPositionModel::from(position.clone()))
            }
            _ => None,
        }
    }
}
