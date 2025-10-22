use std::{ops::Deref, sync::Arc};

use borsh::BorshSerialize;
use gmsol_model::price::Price;
use gmsol_programs::bytemuck;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    market::Value,
    serde::StringPubkey,
    simulation::{
        order::{OrderSimulationOutput, UpdatePriceOptions},
        SimulationOptions, Simulator,
    },
    utils::base64::encode_base64,
};

use super::{
    market::JsMarketModel,
    position::{JsPosition, JsPositionModel},
};

/// A JS binding for [`Simulator`].
#[wasm_bindgen(js_name = Simulator)]
#[derive(Clone)]
pub struct JsSimulator {
    simulator: Simulator,
}

impl From<Simulator> for JsSimulator {
    fn from(simulator: Simulator) -> Self {
        Self { simulator }
    }
}

impl Deref for JsSimulator {
    type Target = Simulator;

    fn deref(&self) -> &Self::Target {
        &self.simulator
    }
}

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

#[wasm_bindgen(js_class = Simulator)]
impl JsSimulator {
    /// Get market by its market token.
    pub fn get_market(&self, market_token: &str) -> crate::Result<Option<JsMarketModel>> {
        Ok(self
            .simulator
            .get_market(&market_token.parse()?)
            .map(|market| market.clone().into()))
    }

    /// Get price for the given token.
    pub fn get_price(&self, token: &str) -> crate::Result<Option<Value>> {
        Ok(self.simulator.get_price(&token.parse()?).map(|p| Value {
            min: p.min,
            max: p.max,
        }))
    }

    /// Upsert the prices for the given token.
    pub fn insert_price(&mut self, token: &str, price: Value) -> crate::Result<()> {
        let token = token.parse()?;
        let price = Arc::new(Price {
            min: price.min,
            max: price.max,
        });
        self.simulator.insert_price(&token, price)?;
        Ok(())
    }

    pub fn simulate_order(
        &mut self,
        args: SimulateOrderArgs,
        position: Option<JsPosition>,
    ) -> crate::Result<JsOrderSimulationOutput> {
        let SimulateOrderArgs {
            kind,
            params,
            collateral_or_swap_out_token,
            pay_token,
            receive_token,
            swap_path,
            prefer_swap_out_token_update,
            skip_limit_price_validation,
            limit_swap_slippage,
            update_prices_for_limit_order,
        } = args;
        let swap_path = swap_path
            .map(|path| path.iter().map(|p| **p).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut simulation = self
            .simulator
            .simulate_order(kind, &params, &collateral_or_swap_out_token)
            .pay_token(pay_token.as_deref())
            .receive_token(receive_token.as_deref())
            .position(position.as_ref().map(|p| &p.position))
            .swap_path(&swap_path)
            .build();

        if update_prices_for_limit_order.unwrap_or_default() {
            simulation = simulation.update_prices(UpdatePriceOptions {
                prefer_swap_in_token_update: !prefer_swap_out_token_update.unwrap_or_default(),
                limit_swap_slippage,
            })?;
        }

        let output = simulation.execute_with_options(SimulationOptions {
            skip_limit_price_validation: skip_limit_price_validation.unwrap_or_default(),
        })?;
        Ok(JsOrderSimulationOutput { output })
    }

    /// Create a clone of this simulator.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
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

fn encode_borsh_base64<T: BorshSerialize>(data: &T) -> crate::Result<String> {
    data.try_to_vec()
        .map(|data| encode_base64(&data))
        .map_err(crate::Error::custom)
}

fn encode_bytemuck_base64<T: bytemuck::NoUninit>(data: &T) -> String {
    encode_base64(bytemuck::bytes_of(data))
}
