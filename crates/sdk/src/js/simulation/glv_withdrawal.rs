//! Withdrawal simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::{
        instructions::create_glv_withdrawal::CreateGlvWithdrawalParamsJs,
        simulation::encode_borsh_base64,
    },
    simulation::glv_withdrawal::GlvWithdrawalSimulationOutput,
};

/// Arguments for GLV withdrawal simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateGlvWithdrawalArgs {
    pub(crate) params: CreateGlvWithdrawalParamsJs,
}

/// Simulation output for withdrawal.
#[wasm_bindgen(js_name = GlvWithdrawalSimulationOutput)]
pub struct JsGlvWithdrawalSimulationOutput {
    pub(crate) output: GlvWithdrawalSimulationOutput,
}

#[wasm_bindgen(js_class = GlvWithdrawalSimulationOutput)]
impl JsGlvWithdrawalSimulationOutput {
    /// Returns the withdraw report.
    pub fn withdraw_report(&self) -> crate::Result<String> {
        encode_borsh_base64(self.output.withdraw_report())
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

    /// Returns long token output amount.
    pub fn long_output_amount(&self) -> u128 {
        self.output.long_output_amount
    }

    /// Returns short token output amount.
    pub fn short_output_amount(&self) -> u128 {
        self.output.short_output_amount
    }
}
