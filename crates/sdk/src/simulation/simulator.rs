use std::{collections::HashMap, sync::Arc};

use gmsol_model::{
    action::swap::SwapReport,
    price::{Price, Prices},
    MarketAction, SwapMarketMutExt,
};
use gmsol_programs::{
    gmsol_store::types::{
        CreateDepositParams, CreateGlvDepositParams, CreateGlvWithdrawalParams, CreateShiftParams,
        CreateWithdrawalParams, MarketMeta,
    },
    model::MarketModel,
};
use solana_sdk::pubkey::Pubkey;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    glv::{calculator::GlvCalculator, model::GlvModel},
    market::caluclator::MarketCalculator,
    simulation::order::OrderSimulation,
};

use super::{
    deposit::{DepositSimulation, DepositSimulationBuilder},
    glv_deposit::{GlvDepositSimulation, GlvDepositSimulationBuilder},
    glv_withdrawal::{GlvWithdrawalSimulation, GlvWithdrawalSimulationBuilder},
    order::OrderSimulationBuilder,
    shift::{ShiftSimulation, ShiftSimulationBuilder},
    withdrawal::{WithdrawalSimulation, WithdrawalSimulationBuilder},
};

/// Order Simulation Builder.
pub type OrderSimulationBuilderForSimulator<'a> = OrderSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (CreateOrderKind,),
        (&'a CreateOrderParams,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

/// Deposit Simulation Builder for Simulator.
pub type DepositSimulationBuilderForSimulator<'a> = DepositSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (&'a CreateDepositParams,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

/// Withdrawal Simulation Builder for Simulator.
pub type WithdrawalSimulationBuilderForSimulator<'a> = WithdrawalSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (&'a CreateWithdrawalParams,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

/// Shift Simulation Builder for Simulator.
pub type ShiftSimulationBuilderForSimulator<'a> = ShiftSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (&'a CreateShiftParams,),
        (&'a Pubkey,),
        (&'a Pubkey,),
    ),
>;

/// GLV Deposit Simulation Builder for Simulator.
pub type GlvDepositSimulationBuilderForSimulator<'a> = GlvDepositSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (&'a CreateGlvDepositParams,),
        (&'a Pubkey,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

/// GLV Withdrawal Simulation Builder for Simulator.
pub type GlvWithdrawalSimulationBuilderForSimulator<'a> = GlvWithdrawalSimulationBuilder<
    'a,
    (
        (&'a mut Simulator,),
        (&'a CreateGlvWithdrawalParams,),
        (&'a Pubkey,),
        (&'a Pubkey,),
        (),
        (),
        (),
        (),
    ),
>;

/// Price State.
pub type PriceState = Option<Arc<Price<u128>>>;

/// A simulator for actions.
#[derive(Debug, Clone)]
pub struct Simulator {
    tokens: HashMap<Pubkey, TokenState>,
    markets: HashMap<Pubkey, MarketModel>,
    glvs: HashMap<Pubkey, GlvModel>,
}

impl Simulator {
    /// Create from parts.
    pub fn from_parts(
        tokens: HashMap<Pubkey, TokenState>,
        markets: HashMap<Pubkey, MarketModel>,
        glvs: HashMap<Pubkey, GlvModel>,
    ) -> Self {
        Self {
            tokens,
            markets,
            glvs,
        }
    }

    /// Get market by its market token.
    pub fn get_market(&self, market_token: &Pubkey) -> Option<&MarketModel> {
        self.markets.get(market_token)
    }

    /// Get market mutably by its market token.
    pub fn get_market_mut(&mut self, market_token: &Pubkey) -> Option<&mut MarketModel> {
        self.markets.get_mut(market_token)
    }

    /// Get prices for the given token.
    pub fn get_price(&self, token: &Pubkey) -> Option<Price<u128>> {
        Some(*self.tokens.get(token)?.price.as_deref()?)
    }

    /// Upsert the prices for the give token.
    ///
    /// # Errors
    /// Returns error if the token does not exist in the simulator.
    pub fn insert_price(
        &mut self,
        token: &Pubkey,
        price: Arc<Price<u128>>,
    ) -> crate::Result<&mut Self> {
        let state = self.tokens.get_mut(token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] token `{token}` is not found in the simulator"
            ))
        })?;
        state.price = Some(price);
        Ok(self)
    }

    /// Get prices for the given market meta.
    pub fn get_prices(&self, meta: &MarketMeta) -> Option<Prices<u128>> {
        let index_token_price = self.get_price(&meta.index_token_mint)?;
        let long_token_price = self.get_price(&meta.long_token_mint)?;
        let short_token_price = self.get_price(&meta.short_token_mint)?;
        Some(Prices {
            index_token_price,
            long_token_price,
            short_token_price,
        })
    }

    pub(crate) fn get_prices_and_meta_for_market(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<(Prices<u128>, &MarketMeta)> {
        let market = self.markets.get(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the simulator"
            ))
        })?;
        let meta = &market.meta;
        let prices = self.get_prices(meta).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] prices for market `{market_token}` are not ready in the simulator"
            ))
        })?;
        Ok((prices, meta))
    }

    pub(crate) fn get_prices_for_market(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<Prices<u128>> {
        Ok(self.get_prices_and_meta_for_market(market_token)?.0)
    }

    pub(crate) fn get_market_with_prices(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<(&MarketModel, Prices<u128>)> {
        let prices = self.get_prices_for_market(market_token)?;
        let market = self.get_market(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the simulator"
            ))
        })?;
        Ok((market, prices))
    }

    pub(crate) fn get_market_with_prices_mut(
        &mut self,
        market_token: &Pubkey,
    ) -> crate::Result<(&mut MarketModel, Prices<u128>)> {
        let prices = self.get_prices_for_market(market_token)?;
        let market = self.get_market_mut(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the simulator"
            ))
        })?;
        Ok((market, prices))
    }

    /// Get GLV by GLV token address.
    pub fn get_glv(&self, glv_token: &Pubkey) -> Option<&GlvModel> {
        self.glvs.get(glv_token)
    }

    /// Get GLV by GLV token address mutably.
    pub fn get_glv_mut(&mut self, glv_token: &Pubkey) -> Option<&mut GlvModel> {
        self.glvs.get_mut(glv_token)
    }

    /// Insert GLV model.
    pub fn insert_glv(&mut self, glv: GlvModel) -> Option<GlvModel> {
        self.glvs.insert(glv.glv_token, glv)
    }

    /// Swap along the provided path.
    pub fn swap_along_path(
        &mut self,
        path: &[Pubkey],
        source_token: &Pubkey,
        mut amount: u128,
    ) -> crate::Result<SwapOutput> {
        let mut current_token = *source_token;

        let mut reports = Vec::with_capacity(path.len());
        for market_token in path {
            let (market, prices) = self.get_market_with_prices_mut(market_token)?;
            let meta = &market.meta;
            if meta.long_token_mint == meta.short_token_mint {
                return Err(crate::Error::custom(format!(
                    "[swap] `{market_token}` is not a swappable market"
                )));
            }
            let is_token_in_long = if meta.long_token_mint == current_token {
                current_token = meta.short_token_mint;
                true
            } else if meta.short_token_mint == current_token {
                current_token = meta.long_token_mint;
                false
            } else {
                return Err(crate::Error::custom(format!(
                    "[swap] invalid swap step. Current step: {market_token}"
                )));
            };
            let report = market.with_vis_disabled(|market| {
                market.swap(is_token_in_long, amount, prices)?.execute()
            })?;
            amount = *report.token_out_amount();
            reports.push(report);
        }

        Ok(SwapOutput {
            output_token: current_token,
            amount,
            reports,
        })
    }

    /// Get token states.
    pub fn tokens(&self) -> impl Iterator<Item = (&Pubkey, &TokenState)> {
        self.tokens.iter()
    }

    /// Get market states.
    pub fn markets(&self) -> impl Iterator<Item = (&Pubkey, &MarketModel)> {
        self.markets.iter()
    }

    /// Get GLV states.
    pub fn glvs(&self) -> impl Iterator<Item = (&Pubkey, &GlvModel)> {
        self.glvs.iter()
    }

    /// Create a builder for order simulation.
    pub fn simulate_order<'a>(
        &'a mut self,
        kind: CreateOrderKind,
        params: &'a CreateOrderParams,
        collateral_or_swap_out_token: &'a Pubkey,
    ) -> OrderSimulationBuilderForSimulator<'a> {
        OrderSimulation::builder()
            .simulator(self)
            .kind(kind)
            .params(params)
            .collateral_or_swap_out_token(collateral_or_swap_out_token)
    }

    /// Create a builder for deposit simulation.
    pub fn simulate_deposit<'a>(
        &'a mut self,
        market_token: &'a Pubkey,
        params: &'a CreateDepositParams,
    ) -> DepositSimulationBuilderForSimulator<'a> {
        DepositSimulation::builder()
            .simulator(self)
            .market_token(market_token)
            .params(params)
    }

    /// Create a builder for withdrawal simulation.
    pub fn simulate_withdrawal<'a>(
        &'a mut self,
        market_token: &'a Pubkey,
        params: &'a CreateWithdrawalParams,
    ) -> WithdrawalSimulationBuilderForSimulator<'a> {
        WithdrawalSimulation::builder()
            .simulator(self)
            .market_token(market_token)
            .params(params)
    }

    /// Create a builder for GLV deposit simulation.
    pub fn simulate_glv_deposit<'a>(
        &'a mut self,
        glv_token: &'a Pubkey,
        market_token: &'a Pubkey,
        params: &'a CreateGlvDepositParams,
    ) -> GlvDepositSimulationBuilderForSimulator<'a> {
        GlvDepositSimulation::builder()
            .simulator(self)
            .glv_token(glv_token)
            .market_token(market_token)
            .params(params)
    }

    /// Create a builder for GLV withdrawal simulation.
    pub fn simulate_glv_withdrawal<'a>(
        &'a mut self,
        glv_token: &'a Pubkey,
        market_token: &'a Pubkey,
        params: &'a CreateGlvWithdrawalParams,
    ) -> GlvWithdrawalSimulationBuilderForSimulator<'a> {
        GlvWithdrawalSimulation::builder()
            .simulator(self)
            .glv_token(glv_token)
            .market_token(market_token)
            .params(params)
    }

    /// Create a builder for shift simulation.
    pub fn simulate_shift<'a>(
        &'a mut self,
        from_market_token: &'a Pubkey,
        to_market_token: &'a Pubkey,
        params: &'a CreateShiftParams,
    ) -> ShiftSimulationBuilderForSimulator<'a> {
        ShiftSimulation::builder()
            .simulator(self)
            .from_market_token(from_market_token)
            .to_market_token(to_market_token)
            .params(params)
    }
}

/// Options for simulation.
#[derive(Debug, Default, Clone)]
pub struct SimulationOptions {
    /// Whether to skip the validation for limit price.
    pub skip_limit_price_validation: bool,
}

/// Token state for [`Simulator`].
#[derive(Debug, Clone)]
pub struct TokenState {
    price: PriceState,
}

impl TokenState {
    /// Create from [`PriceState`].
    pub fn from_price(price: PriceState) -> Self {
        Self { price }
    }

    /// Get price state.
    pub fn price(&self) -> &PriceState {
        &self.price
    }
}

/// Swap output.
#[derive(Debug, Clone)]
pub struct SwapOutput {
    pub(crate) output_token: Pubkey,
    pub(crate) amount: u128,
    pub(crate) reports: Vec<SwapReport<u128, i128>>,
}

impl SwapOutput {
    /// Returns the output token.
    pub fn output_token(&self) -> &Pubkey {
        &self.output_token
    }

    /// Returns the output amount.
    pub fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the swap reports.
    pub fn reports(&self) -> &[SwapReport<u128, i128>] {
        &self.reports
    }
}

impl MarketCalculator for Simulator {
    fn get_market_model(&self, market_token: &Pubkey) -> Option<&MarketModel> {
        self.get_market(market_token)
    }

    fn get_token_price(&self, token: &Pubkey) -> Option<Price<u128>> {
        self.get_price(token)
    }
}

impl GlvCalculator for Simulator {
    fn get_glv_model(&self, glv_token: &Pubkey) -> Option<&GlvModel> {
        self.get_glv(glv_token)
    }
}
