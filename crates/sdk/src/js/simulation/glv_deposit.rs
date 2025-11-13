//! Deposit simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::{
        instructions::create_glv_deposit::CreateGlvDepositParamsJs, simulation::encode_borsh_base64,
    },
    simulation::glv_deposit::GlvDepositSimulationOutput,
};

/// Arguments for GLV deposit simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateGlvDepositArgs {
    pub(crate) params: CreateGlvDepositParamsJs,
}

/// Simulation output for GLV deposit.
#[wasm_bindgen(js_name = GlvDepositSimulationOutput)]
pub struct JsGlvDepositSimulationOutput {
    pub(crate) output: GlvDepositSimulationOutput,
}

#[wasm_bindgen(js_class = GlvDepositSimulationOutput)]
impl JsGlvDepositSimulationOutput {
    /// Returns the deposit report.
    pub fn deposit_report(&self) -> crate::Result<Option<String>> {
        self.output
            .deposit_report()
            .map(encode_borsh_base64)
            .transpose()
    }

    /// Returns swap reports for the long token path.
    pub fn long_swaps(&self) -> crate::Result<Vec<String>> {
        self.output
            .long_swaps()
            .iter()
            .map(encode_borsh_base64)
            .collect()
    }

    /// Returns swap reports for the short token path.
    pub fn short_swaps(&self) -> crate::Result<Vec<String>> {
        self.output
            .short_swaps()
            .iter()
            .map(encode_borsh_base64)
            .collect()
    }

    /// Returns the output GLV token amount.
    pub fn output_amount(&self) -> u128 {
        self.output.output_amount().into()
    }
}
