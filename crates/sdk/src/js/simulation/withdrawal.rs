//! Withdrawal simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    js::{
        instructions::create_withdrawal::CreateWithdrawalParamsJs, simulation::encode_borsh_base64,
    },
    serde::StringPubkey,
    simulation::withdrawal::WithdrawalSimulationOutput,
};

/// Arguments for withdrawal simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateWithdrawalArgs {
    pub(crate) params: CreateWithdrawalParamsJs,
    pub(crate) market_token: StringPubkey,
    #[serde(default)]
    pub(crate) long_receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) short_receive_token: Option<StringPubkey>,
}

/// Simulation output for withdrawal.
#[wasm_bindgen(js_name = WithdrawalSimulationOutput)]
pub struct JsWithdrawalSimulationOutput {
    pub(crate) output: WithdrawalSimulationOutput,
}

#[wasm_bindgen(js_class = WithdrawalSimulationOutput)]
impl JsWithdrawalSimulationOutput {
    /// Returns the withdraw report.
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

    /// Returns long token output amount.
    pub fn long_output_amount(&self) -> u128 {
        self.output.long_output_amount
    }

    /// Returns short token output amount.
    pub fn short_output_amount(&self) -> u128 {
        self.output.short_output_amount
    }
}
