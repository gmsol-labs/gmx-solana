//! Shift simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::{instructions::create_shift::CreateShiftParamsJs, simulation::encode_borsh_base64},
    simulation::shift::ShiftSimulationOutput,
};

/// Arguments for shift simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateShiftArgs {
    pub(crate) params: CreateShiftParamsJs,
}

/// Simulation output for shift.
#[wasm_bindgen(js_name = ShiftSimulationOutput)]
pub struct JsShiftSimulationOutput {
    pub(crate) output: ShiftSimulationOutput,
}

#[wasm_bindgen(js_class = ShiftSimulationOutput)]
impl JsShiftSimulationOutput {
    /// Returns the deposit report.
    pub fn deposit_report(&self) -> crate::Result<String> {
        encode_borsh_base64(self.output.deposit_report())
    }

    /// Returns the withdraw report.
    pub fn withdraw_report(&self) -> crate::Result<String> {
        encode_borsh_base64(self.output.withdraw_report())
    }
}
