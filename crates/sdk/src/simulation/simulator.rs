use std::{collections::HashMap, sync::Arc};

use gmsol_model::{
    action::swap::SwapReport,
    price::{Price, Prices},
    MarketAction, SwapMarketMutExt,
};
use gmsol_programs::{gmsol_store::types::MarketMeta, model::MarketModel};
use solana_sdk::pubkey::Pubkey;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    simulation::order::OrderSimulation,
};

use super::order::OrderSimulationBuilder;

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

/// Price State.
pub type PriceState = Option<Arc<Price<u128>>>;

/// A simulator for actions.
#[derive(Debug, Clone)]
pub struct Simulator {
    tokens: HashMap<Pubkey, TokenState>,
    markets: HashMap<Pubkey, MarketModel>,
}

impl Simulator {
    /// Create from parts.
    pub fn from_parts(
        tokens: HashMap<Pubkey, TokenState>,
        markets: HashMap<Pubkey, MarketModel>,
    ) -> Self {
        Self { tokens, markets }
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

    pub(crate) fn get_prices_for_market(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<Prices<u128>> {
        let market = self.markets.get(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the simulator"
            ))
        })?;
        let meta = &market.meta;
        self.get_prices(meta).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] prices for market `{market_token}` are not ready in the simulator"
            ))
        })
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
            let report = market.swap(is_token_in_long, amount, prices)?.execute()?;
            amount = *report.token_out_amount();
            reports.push(report);
        }

        Ok(SwapOutput {
            output_token: current_token,
            amount,
            reports,
        })
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

    /// Get token states.
    pub fn tokens(&self) -> impl Iterator<Item = (&Pubkey, &TokenState)> {
        self.tokens.iter()
    }

    /// Get market states.
    pub fn markets(&self) -> impl Iterator<Item = (&Pubkey, &MarketModel)> {
        self.markets.iter()
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
