use std::sync::Arc;

use crate::{
    js::{position::JsPosition, simulation::JsOrderSimulationOutput},
    market::Value,
    market_graph::{
        CreateGraphSimulatorOptions, MarketGraph, MarketGraphConfig, SwapEstimationParams,
        UpdateGraphWithSimulatorOptions,
    },
    serde::StringPubkey,
    utils::{base64::encode_base64, zero_copy::try_deserialize_zero_copy_from_base64},
};

use borsh::BorshSerialize;
use gmsol_programs::model::MarketModel;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use super::{
    market::JsMarketModel,
    simulation::{JsSimulator, SimulateOrderArgs},
};

/// A JS binding for [`MarketGraph`].
#[wasm_bindgen(js_name = MarketGraph)]
#[derive(Clone)]
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
    #[deprecated(since = "0.8.0", note = "use `to_simulator` instead")]
    #[allow(deprecated)]
    pub fn simulate_order(
        &self,
        args: SimulateOrderArgs,
        position: Option<JsPosition>,
    ) -> crate::Result<JsOrderSimulationOutput> {
        use crate::market_graph::simulation::SimulationOptions;

        let SimulateOrderArgs {
            kind,
            params,
            collateral_or_swap_out_token,
            pay_token,
            receive_token,
            swap_path,
            prefer_swap_out_token_update,
            ..
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

    pub fn simulate_deposit_swaps(&self, args: LpSwapLegsArgs) -> crate::Result<LpSwapLegsOutput> {
        self.simulate_lp_swap_legs(args)
    }

    pub fn simulate_withdrawal_swaps(
        &self,
        args: LpSwapLegsArgs,
    ) -> crate::Result<LpSwapLegsOutput> {
        self.simulate_lp_swap_legs(args)
    }

    pub fn simulate_glv_deposit_swaps(
        &self,
        args: LpSwapLegsArgs,
    ) -> crate::Result<LpSwapLegsOutput> {
        self.simulate_lp_swap_legs(args)
    }

    pub fn simulate_glv_withdrawal_swaps(
        &self,
        args: LpSwapLegsArgs,
    ) -> crate::Result<LpSwapLegsOutput> {
        self.simulate_lp_swap_legs(args)
    }

    fn simulate_lp_swap_legs(&self, args: LpSwapLegsArgs) -> crate::Result<LpSwapLegsOutput> {
        use crate::simulation::Simulator;

        let simulator: Simulator = self.graph.to_simulator(Default::default());
        let mut simulator = simulator.clone();

        let long = if let Some(leg) = args.long {
            Some(simulate_swap_leg(&mut simulator, &leg)?)
        } else {
            None
        };

        let short = if let Some(leg) = args.short {
            Some(simulate_swap_leg(&mut simulator, &leg)?)
        } else {
            None
        };

        Ok(LpSwapLegsOutput { long, short })
    }

    pub fn simulate_deposit_exact(
        &self,
        args: GmDepositExactArgs,
    ) -> crate::Result<GmDepositExactOutput> {
        use crate::simulation::{simulate_gm_deposit_exact, Simulator};
        let simulator: Simulator = self.graph.to_simulator(Default::default());
        let mut simulator = simulator.clone();
        let market_token = *args.market_token;
        let out = simulate_gm_deposit_exact(
            &mut simulator,
            &market_token,
            args.long_amount,
            args.short_amount,
            args.include_virtual_inventory_impact.unwrap_or(true),
        )?;
        Ok(GmDepositExactOutput {
            minted: out.minted,
            report: out.report_borsh_base64,
        })
    }

    pub fn simulate_withdrawal_exact(
        &self,
        args: GmWithdrawalExactArgs,
    ) -> crate::Result<GmWithdrawalExactOutput> {
        use crate::simulation::{simulate_gm_withdrawal_exact, Simulator};
        let simulator: Simulator = self.graph.to_simulator(Default::default());
        let mut simulator = simulator.clone();
        let market_token = *args.market_token;
        let out =
            simulate_gm_withdrawal_exact(&mut simulator, &market_token, args.market_token_amount)?;
        Ok(GmWithdrawalExactOutput {
            long_out: out.long_out,
            short_out: out.short_out,
            report: out.report_borsh_base64,
        })
    }

    pub fn simulate_glv_deposit_exact(
        &self,
        args: GlvDepositExactArgs,
    ) -> crate::Result<GlvDepositExactOutput> {
        use crate::simulation::{self, Simulator};
        use solana_sdk::pubkey::Pubkey;

        let simulator: Simulator = self.graph.to_simulator(Default::default());
        let mut simulator = simulator.clone();
        let market_token: Pubkey = args.market_token.into();
        let components = args
            .components
            .iter()
            .map(|c| simulation::lp::GlvComponentSim {
                market_token: c.market_token.into(),
                balance: c.balance,
                pool_value: c.pool_value,
                supply: c.supply,
            })
            .collect::<Vec<_>>();

        let out = simulation::lp::simulate_glv_deposit_exact(
            &mut simulator,
            &market_token,
            args.long_amount,
            args.short_amount,
            args.market_token_amount,
            args.include_virtual_inventory_impact.unwrap_or(true),
            args.glv_supply,
            &components,
        )?;

        Ok(GlvDepositExactOutput {
            glv_minted: out.glv_minted,
            market_token_delta: out.market_token_delta,
            pricing_report: out.pricing_report_borsh_base64,
        })
    }

    pub fn simulate_glv_withdrawal_exact(
        &self,
        args: GlvWithdrawalExactArgs,
    ) -> crate::Result<GlvWithdrawalExactOutput> {
        use crate::simulation::{self, Simulator};
        use solana_sdk::pubkey::Pubkey;

        let simulator: Simulator = self.graph.to_simulator(Default::default());
        let mut simulator = simulator.clone();
        let market_token: Pubkey = *args.market_token;
        let components = args
            .components
            .iter()
            .map(|c| simulation::lp::GlvComponentSim {
                market_token: c.market_token.into(),
                balance: c.balance,
                pool_value: c.pool_value,
                supply: c.supply,
            })
            .collect::<Vec<_>>();

        let out = simulation::lp::simulate_glv_withdrawal_exact(
            &mut simulator,
            &market_token,
            args.glv_token_amount,
            args.glv_supply,
            &components,
        )?;

        Ok(GlvWithdrawalExactOutput {
            market_token_amount: out.market_token_amount,
            long_out: out.long_out,
            short_out: out.short_out,
            pricing_report: out.pricing_report_borsh_base64,
        })
    }

    /// Create a simulator.
    pub fn to_simulator(&self, options: Option<CreateGraphSimulatorOptions>) -> JsSimulator {
        JsSimulator::from(self.graph.to_simulator(options.unwrap_or_default()))
    }

    /// Update with simulator.
    pub fn update_with_simulator(
        &mut self,
        simulator: &JsSimulator,
        options: Option<UpdateGraphWithSimulatorOptions>,
    ) -> crate::Result<()> {
        self.graph
            .update_with_simulator(simulator, options.unwrap_or_default());
        Ok(())
    }

    /// Create a clone of this graph.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

fn encode_borsh_base64<T: BorshSerialize>(data: &T) -> crate::Result<String> {
    data.try_to_vec()
        .map(|data| encode_base64(&data))
        .map_err(crate::Error::custom)
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct LpSwapLegArgs {
    pub source_token: StringPubkey,
    pub amount: u128,
    #[serde(default)]
    pub swap_path: Vec<StringPubkey>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct LpSwapLegsArgs {
    #[serde(default)]
    pub long: Option<LpSwapLegArgs>,
    #[serde(default)]
    pub short: Option<LpSwapLegArgs>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct LpSwapLegOutput {
    pub output_token: StringPubkey,
    pub amount: u128,
    pub report: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct LpSwapLegsOutput {
    #[serde(default)]
    pub long: Option<LpSwapLegOutput>,
    #[serde(default)]
    pub short: Option<LpSwapLegOutput>,
}

fn simulate_swap_leg(
    sim: &mut crate::simulation::Simulator,
    leg: &LpSwapLegArgs,
) -> crate::Result<LpSwapLegOutput> {
    use solana_sdk::pubkey::Pubkey;
    let source: Pubkey = *leg.source_token;
    let path: Vec<Pubkey> = leg.swap_path.iter().map(|p| **p).collect();
    let swap = sim.swap_along_path(&path, &source, leg.amount)?;
    Ok(LpSwapLegOutput {
        output_token: (*swap.output_token()).into(),
        amount: swap.amount(),
        report: swap
            .reports()
            .iter()
            .map(encode_borsh_base64)
            .collect::<crate::Result<Vec<_>>>()?,
    })
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GmDepositExactArgs {
    pub market_token: StringPubkey,
    pub long_amount: u128,
    pub short_amount: u128,
    #[serde(default)]
    pub include_virtual_inventory_impact: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct GmDepositExactOutput {
    pub minted: u128,
    pub report: String,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GmWithdrawalExactArgs {
    pub market_token: StringPubkey,
    pub market_token_amount: u128,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct GmWithdrawalExactOutput {
    pub long_out: u128,
    pub short_out: u128,
    pub report: String,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GlvComponent {
    pub market_token: StringPubkey,
    pub balance: u64,
    pub pool_value: u128,
    pub supply: u64,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GlvDepositExactArgs {
    pub market_token: StringPubkey,
    pub long_amount: u128,
    pub short_amount: u128,
    pub market_token_amount: u128,
    pub glv_supply: u64,
    pub components: Vec<GlvComponent>,
    #[serde(default)]
    pub include_virtual_inventory_impact: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct GlvDepositExactOutput {
    pub glv_minted: u64,
    pub market_token_delta: u64,
    pub pricing_report: String,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct GlvWithdrawalExactArgs {
    pub market_token: StringPubkey,
    pub glv_token_amount: u64,
    pub glv_supply: u64,
    pub components: Vec<GlvComponent>,
}

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi)]
pub struct GlvWithdrawalExactOutput {
    pub market_token_amount: u64,
    pub long_out: u128,
    pub short_out: u128,
    pub pricing_report: String,
}
