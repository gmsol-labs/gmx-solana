use std::sync::Arc;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    js::position::JsPosition,
    market::Value,
    market_graph::{
        simulation::{order::OrderSimulationOutput, SimulationOptions},
        MarketGraph, MarketGraphConfig, SwapEstimationParams,
    },
    serde::StringPubkey,
    utils::{base64::encode_base64, zero_copy::try_deserialize_zero_copy_from_base64},
};
use borsh::BorshSerialize;
use gmsol_programs::{bytemuck, model::MarketModel};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use super::market::JsMarketModel;

/// A JS binding for [`MarketGraph`].
#[wasm_bindgen(js_name = MarketGraph)]
pub struct JsMarketGraph {
    graph: MarketGraph,
}

/// Best swap path.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct BestSwapPath {
    /// Params.
    pub params: SwapEstimationParams,
    /// Exchange rate.
    pub exchange_rate: Option<u128>,
    /// Path.
    pub path: Vec<String>,
    /// Arbitrage exists.
    pub arbitrage_exists: Option<bool>,
}

/// Arguments for order simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateOrderArgs {
    kind: CreateOrderKind,
    params: CreateOrderParams,
    collateral_or_swap_out_token: StringPubkey,
    #[serde(default)]
    pay_token: Option<StringPubkey>,
    #[serde(default)]
    receive_token: Option<StringPubkey>,
    #[serde(default)]
    swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    prefer_swap_out_token_update: Option<bool>,
}

#[wasm_bindgen(js_class = MarketGraph)]
impl JsMarketGraph {
    /// Create an empty market graph.
    #[wasm_bindgen(constructor)]
    pub fn new(config: MarketGraphConfig) -> Self {
        Self {
            graph: MarketGraph::with_config(config),
        }
    }

    /// Insert market from base64 encoded data.
    pub fn insert_market_from_base64(&mut self, data: &str, supply: u64) -> crate::Result<bool> {
        let market = try_deserialize_zero_copy_from_base64(data)?.0;
        let model = MarketModel::from_parts(Arc::new(market), supply);
        Ok(self.graph.insert_market(model))
    }

    /// Update token price.
    pub fn update_token_price(&mut self, token: &str, price: Value) -> crate::Result<()> {
        self.graph
            .update_token_price(&token.parse()?, &price.into());
        Ok(())
    }

    /// Update value.
    pub fn update_value(&mut self, value: u128) {
        self.graph.update_value(value);
    }

    /// Update base cost.
    pub fn update_base_cost(&mut self, base_cost: u128) {
        self.graph.update_base_cost(base_cost);
    }

    /// Update max steps.
    pub fn update_max_steps(&mut self, max_steps: usize) {
        self.graph.update_max_steps(max_steps);
    }

    /// Get market by its market token.
    pub fn get_market(&self, market_token: &str) -> crate::Result<Option<JsMarketModel>> {
        Ok(self
            .graph
            .get_market(&market_token.parse()?)
            .map(|market| market.clone().into()))
    }

    /// Get all market tokens.
    pub fn market_tokens(&self) -> Vec<String> {
        self.graph
            .market_tokens()
            .map(|token| token.to_string())
            .collect()
    }

    /// Get all index tokens.
    pub fn index_tokens(&self) -> Vec<String> {
        self.graph
            .index_tokens()
            .map(|token| token.to_string())
            .collect()
    }

    /// Compute best swap path.
    pub fn best_swap_path(
        &self,
        source: &str,
        target: &str,
        skip_bellman_ford: bool,
    ) -> crate::Result<BestSwapPath> {
        let target = target.parse()?;
        let paths = self
            .graph
            .best_swap_paths(&source.parse()?, skip_bellman_ford)?;
        let arbitrage_exists = paths.arbitrage_exists();
        let (exchange_rate, path) = paths.to(&target);
        Ok(BestSwapPath {
            params: *paths.params(),
            exchange_rate: exchange_rate.map(|d| {
                d.round_dp(crate::constants::MARKET_DECIMALS as u32)
                    .mantissa()
                    .try_into()
                    .unwrap()
            }),
            path: path.into_iter().map(|token| token.to_string()).collect(),
            arbitrage_exists,
        })
    }

    /// Simulates order execution.
    pub fn simulate_order(
        &self,
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
        } = args;
        let swap_path = swap_path
            .map(|path| path.iter().map(|p| **p).collect::<Vec<_>>())
            .unwrap_or_default();
        let output = self
            .graph
            .simulate_order(kind, &params, &collateral_or_swap_out_token)
            .pay_token(pay_token.as_deref())
            .receive_token(receive_token.as_deref())
            .position(position.as_ref().map(|p| &p.position))
            .swap_path(&swap_path)
            .build()
            .execute_with_options(SimulationOptions {
                prefer_swap_in_token_update: !prefer_swap_out_token_update.unwrap_or_default(),
            })?;
        Ok(JsOrderSimulationOutput { output })
    }
}

/// A JS binding for [`OrderSimulationOutput`].
#[wasm_bindgen(js_name = OrderSimulationOutput)]
pub struct JsOrderSimulationOutput {
    output: OrderSimulationOutput,
}

/// Simulation output for increase order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct IncreaseOrderSimulationOutput {
    swaps: Vec<String>,
    report: String,
    position: String,
}

/// Simulation output for decrease order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DecreaseOrderSimulationOutput {
    swaps: Vec<String>,
    report: String,
    position: String,
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
    pub fn increase(&self) -> crate::Result<Option<IncreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Increase {
            swaps,
            report,
            position,
        } = &self.output
        {
            Ok(Some(IncreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_bytemuck_base64(position.position()),
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns decrease order simulation output.
    pub fn decrease(&self) -> crate::Result<Option<DecreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Decrease {
            swaps,
            report,
            position,
        } = &self.output
        {
            Ok(Some(DecreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_bytemuck_base64(position.position()),
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
}

fn encode_borsh_base64<T: BorshSerialize>(data: &T) -> crate::Result<String> {
    data.try_to_vec()
        .map(|data| encode_base64(&data))
        .map_err(crate::Error::custom)
}

fn encode_bytemuck_base64<T: bytemuck::NoUninit>(data: &T) -> String {
    encode_base64(bytemuck::bytes_of(data))
}
