//! JS binding for [`Simulator`].

use std::{ops::Deref, sync::Arc};

use gmsol_model::price::Price;
use gmsol_programs::gmsol_store::types::{
    CreateDepositParams, CreateGlvDepositParams, CreateGlvWithdrawalParams, CreateShiftParams,
    CreateWithdrawalParams,
};
use solana_sdk::pubkey::Pubkey;
use wasm_bindgen::prelude::*;

use crate::{
    js::glv::JsGlvModel,
    market::Value,
    serde::StringPubkey,
    simulation::{order::UpdatePriceOptions, SimulationOptions, Simulator},
};

use crate::js::{market::JsMarketModel, position::JsPosition};

use super::{
    deposit::{JsDepositSimulationOutput, SimulateDepositArgs},
    glv_deposit::{JsGlvDepositSimulationOutput, SimulateGlvDepositArgs},
    glv_withdrawal::{JsGlvWithdrawalSimulationOutput, SimulateGlvWithdrawalArgs},
    order::{JsOrderSimulationOutput, SimulateOrderArgs},
    shift::{JsShiftSimulationOutput, SimulateShiftArgs},
    withdrawal::{JsWithdrawalSimulationOutput, SimulateWithdrawalArgs},
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

    /// Get GLV by its GLV token.
    pub fn get_glv(&self, glv_token: &str) -> crate::Result<Option<JsGlvModel>> {
        Ok(self
            .simulator
            .get_glv(&glv_token.parse()?)
            .map(|glv| glv.clone().into()))
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

    /// Upsert a GLV model.
    pub fn insert_glv(&mut self, glv: &JsGlvModel) -> crate::Result<()> {
        self.simulator.insert_glv(glv.model.clone());
        Ok(())
    }

    /// Simulate an order execution.
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
        let swap_path = convert_swap_path(swap_path.as_deref());
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

    /// Simulate a deposit execution.
    pub fn simulate_deposit(
        &mut self,
        args: SimulateDepositArgs,
    ) -> crate::Result<JsDepositSimulationOutput> {
        let SimulateDepositArgs { params } = args;

        let long_swap_path = convert_swap_path(params.long_swap_path.as_deref());
        let short_swap_path = convert_swap_path(params.short_swap_path.as_deref());
        let market_token = &params.market_token;
        let long_pay_token = &params.long_pay_token;
        let short_pay_token = &params.short_pay_token;
        let params = CreateDepositParams {
            execution_lamports: 0,
            long_token_swap_length: long_swap_path.len().try_into()?,
            short_token_swap_length: short_swap_path.len().try_into()?,
            initial_long_token_amount: params.long_pay_amount.unwrap_or_default().try_into()?,
            initial_short_token_amount: params.short_pay_amount.unwrap_or_default().try_into()?,
            min_market_token_amount: params.min_receive_amount.unwrap_or_default().try_into()?,
            should_unwrap_native_token: !params.skip_unwrap_native_on_receive.unwrap_or_default(),
        };

        let output = self
            .simulator
            .simulate_deposit(market_token, &params)
            .long_pay_token(long_pay_token.as_deref())
            .long_swap_path(&long_swap_path)
            .short_pay_token(short_pay_token.as_deref())
            .short_swap_path(&short_swap_path)
            .build()
            .execute_with_options(Default::default())?;

        Ok(JsDepositSimulationOutput { output })
    }

    /// Simulate a withdrawal execution.
    pub fn simulate_withdrawal(
        &mut self,
        args: SimulateWithdrawalArgs,
    ) -> crate::Result<JsWithdrawalSimulationOutput> {
        let SimulateWithdrawalArgs { params } = args;

        let long_swap_path = convert_swap_path(params.long_swap_path.as_deref());
        let short_swap_path = convert_swap_path(params.short_swap_path.as_deref());
        let market_token = &params.market_token;
        let long_receive_token = &params.long_receive_token;
        let short_receive_token = &params.short_receive_token;
        let params = CreateWithdrawalParams {
            execution_lamports: 0,
            long_token_swap_path_length: long_swap_path.len().try_into()?,
            short_token_swap_path_length: short_swap_path.len().try_into()?,
            market_token_amount: params.market_token_amount.unwrap_or_default().try_into()?,
            min_long_token_amount: params
                .min_long_receive_amount
                .unwrap_or_default()
                .try_into()?,
            min_short_token_amount: params
                .min_short_receive_amount
                .unwrap_or_default()
                .try_into()?,
            should_unwrap_native_token: !params.skip_unwrap_native_on_receive.unwrap_or_default(),
        };

        let output = self
            .simulator
            .simulate_withdrawal(market_token, &params)
            .long_receive_token(long_receive_token.as_deref())
            .long_swap_path(&long_swap_path)
            .short_receive_token(short_receive_token.as_deref())
            .short_swap_path(&short_swap_path)
            .build()
            .execute_with_options(Default::default())?;

        Ok(JsWithdrawalSimulationOutput { output })
    }

    /// Simulate a shift execution.
    pub fn simulate_shift(
        &mut self,
        args: SimulateShiftArgs,
    ) -> crate::Result<JsShiftSimulationOutput> {
        let SimulateShiftArgs { params } = args;

        let from_market_token = &params.from_market_token;
        let to_market_token = &params.to_market_token;
        let params = CreateShiftParams {
            execution_lamports: 0,
            from_market_token_amount: params
                .from_market_token_amount
                .unwrap_or_default()
                .try_into()?,
            min_to_market_token_amount: params
                .min_to_market_token_amount
                .unwrap_or_default()
                .try_into()?,
        };

        let output = self
            .simulator
            .simulate_shift(from_market_token, to_market_token, &params)
            .build()
            .execute_with_options(Default::default())?;

        Ok(JsShiftSimulationOutput { output })
    }

    /// Simulate a GLV deposit execution.
    pub fn simulate_glv_deposit(
        &mut self,
        args: SimulateGlvDepositArgs,
    ) -> crate::Result<JsGlvDepositSimulationOutput> {
        let SimulateGlvDepositArgs { params } = args;

        let long_swap_path = convert_swap_path(params.long_swap_path.as_deref());
        let short_swap_path = convert_swap_path(params.short_swap_path.as_deref());
        let glv_token = &params.glv_token;
        let market_token = &params.market_token;
        let long_pay_token = &params.long_pay_token;
        let short_pay_token = &params.short_pay_token;
        let params = CreateGlvDepositParams {
            execution_lamports: 0,
            long_token_swap_length: long_swap_path.len().try_into()?,
            short_token_swap_length: short_swap_path.len().try_into()?,
            initial_long_token_amount: params.long_pay_amount.unwrap_or_default().try_into()?,
            initial_short_token_amount: params.short_pay_amount.unwrap_or_default().try_into()?,
            market_token_amount: params.market_token_amount.unwrap_or_default().try_into()?,
            min_market_token_amount: params
                .min_market_token_amount
                .unwrap_or_default()
                .try_into()?,
            min_glv_token_amount: params.min_receive_amount.unwrap_or_default().try_into()?,
            should_unwrap_native_token: !params.skip_unwrap_native_on_receive.unwrap_or_default(),
        };

        let output = self
            .simulator
            .simulate_glv_deposit(glv_token, market_token, &params)
            .long_pay_token(long_pay_token.as_deref())
            .long_swap_path(&long_swap_path)
            .short_pay_token(short_pay_token.as_deref())
            .short_swap_path(&short_swap_path)
            .build()
            .execute_with_options(Default::default())?;

        Ok(JsGlvDepositSimulationOutput { output })
    }

    /// Simulate a GLV withdrawal execution.
    pub fn simulate_glv_withdrawal(
        &mut self,
        args: SimulateGlvWithdrawalArgs,
    ) -> crate::Result<JsGlvWithdrawalSimulationOutput> {
        let SimulateGlvWithdrawalArgs { params } = args;

        let long_swap_path = convert_swap_path(params.long_swap_path.as_deref());
        let short_swap_path = convert_swap_path(params.short_swap_path.as_deref());
        let glv_token = &params.glv_token;
        let market_token = &params.market_token;
        let long_receive_token = &params.long_receive_token;
        let short_receive_token = &params.short_receive_token;
        let params = CreateGlvWithdrawalParams {
            execution_lamports: 0,
            long_token_swap_length: long_swap_path.len().try_into()?,
            short_token_swap_length: short_swap_path.len().try_into()?,
            glv_token_amount: params.glv_token_amount.unwrap_or_default().try_into()?,
            min_final_long_token_amount: params
                .min_long_receive_amount
                .unwrap_or_default()
                .try_into()?,
            min_final_short_token_amount: params
                .min_short_receive_amount
                .unwrap_or_default()
                .try_into()?,
            should_unwrap_native_token: !params.skip_unwrap_native_on_receive.unwrap_or_default(),
        };

        let output = self
            .simulator
            .simulate_glv_withdrawal(glv_token, market_token, &params)
            .long_receive_token(long_receive_token.as_deref())
            .long_swap_path(&long_swap_path)
            .short_receive_token(short_receive_token.as_deref())
            .short_swap_path(&short_swap_path)
            .build()
            .execute_with_options(Default::default())?;

        Ok(JsGlvWithdrawalSimulationOutput { output })
    }

    /// Create a clone of this simulator.
    #[wasm_bindgen(js_name = clone)]
    pub fn js_clone(&self) -> Self {
        self.clone()
    }
}

fn convert_swap_path(swap_path: Option<&[StringPubkey]>) -> Vec<Pubkey> {
    swap_path
        .map(|path| path.iter().map(|p| **p).collect::<Vec<_>>())
        .unwrap_or_default()
}
