//! Deposit simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::{instructions::create_deposit::CreateDepositParamsJs, simulation::encode_borsh_base64},
    serde::StringPubkey,
    simulation::deposit::DepositSimulationOutput,
};

/// Arguments for deposit simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateDepositArgs {
    pub(crate) params: CreateDepositParamsJs,
    pub(crate) market_token: StringPubkey,
    #[serde(default)]
    pub(crate) long_pay_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) short_pay_token: Option<StringPubkey>,
}

/// Simulation output for deposit.
#[wasm_bindgen(js_name = DepositSimulationOutput)]
pub struct JsDepositSimulationOutput {
    pub(crate) output: DepositSimulationOutput,
}

#[wasm_bindgen(js_class = DepositSimulationOutput)]
impl JsDepositSimulationOutput {
    /// Returns the deposit report.
    pub fn report(&self) -> crate::Result<String> {
        encode_borsh_base64(self.output.report())
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
}
