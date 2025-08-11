use std::{collections::HashMap, sync::Arc};

use gmsol_model::{
    action::{
        decrease_position::{DecreasePositionFlags, DecreasePositionReport},
        increase_position::IncreasePositionReport,
        swap::SwapReport,
    },
    num::MulDiv,
    price::{Price, Prices},
    MarketAction, PositionMutExt,
};
use gmsol_programs::{
    gmsol_store::accounts::Position,
    model::{MarketModel, PositionModel},
};
use rust_decimal::prelude::Zero;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    market_graph::{simulation::SimulationOptions, MarketGraph, SwapOutput},
};

/// Order simulation output.
#[derive(Debug)]
pub enum OrderSimulationOutput {
    /// Increase output.
    Increase {
        swaps: Vec<SwapReport<u128, i128>>,
        report: Box<IncreasePositionReport<u128, i128>>,
        position: PositionModel,
    },
    /// Decrease output.
    Decrease {
        swaps: Vec<SwapReport<u128, i128>>,
        report: Box<DecreasePositionReport<u128, i128>>,
        position: PositionModel,
    },
    /// Swap output.
    Swap(SwapOutput),
}

/// Order execution simulation.
#[derive(Debug, Clone, TypedBuilder)]
pub struct OrderSimulation<'a> {
    graph: &'a MarketGraph,
    kind: CreateOrderKind,
    params: &'a CreateOrderParams,
    collateral_or_swap_out_token: &'a Pubkey,
    #[builder(default)]
    pay_token: Option<&'a Pubkey>,
    #[builder(default)]
    receive_token: Option<&'a Pubkey>,
    #[builder(default)]
    swap_path: &'a [Pubkey],
    #[builder(default)]
    position: Option<&'a Arc<Position>>,
}

impl OrderSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        options: SimulationOptions,
    ) -> crate::Result<OrderSimulationOutput> {
        match self.kind {
            CreateOrderKind::MarketIncrease | CreateOrderKind::LimitIncrease => self.increase(),
            CreateOrderKind::MarketDecrease
            | CreateOrderKind::LimitDecrease
            | CreateOrderKind::StopLossDecrease => self.decrease(),
            CreateOrderKind::MarketSwap | CreateOrderKind::LimitSwap => self.swap(options),
        }
    }

    fn market_model_with_prices(&self) -> crate::Result<(MarketModel, Prices<u128>)> {
        let market_token = &self.params.market_token;
        let model = self.graph.get_market(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the graph"
            ))
        })?;
        let prices = self.graph.get_prices(&model.meta).ok_or_else(|| {
            crate::Error::custom(format!("[sim] prices for `{market_token}` are not ready"))
        })?;

        Ok((model.clone(), prices))
    }

    fn increase(self) -> crate::Result<OrderSimulationOutput> {
        let (market, mut prices) = self.market_model_with_prices()?;

        let Self {
            kind,
            graph,
            params,
            collateral_or_swap_out_token,
            position,
            swap_path,
            pay_token,
            ..
        } = self;

        if matches!(kind, CreateOrderKind::LimitIncrease) {
            let Some(trigger_price) = params.trigger_price else {
                return Err(crate::Error::custom("[sim] trigger price is required"));
            };
            let price = Price {
                min: trigger_price,
                max: trigger_price,
            };
            // NOTE: Collateral token price update not supported yet; may be in future.
            prices.index_token_price = price;
        }

        let source_token = pay_token.unwrap_or(collateral_or_swap_out_token);
        let swap_output = graph.swap_along_path(swap_path, source_token, params.amount)?;
        if swap_output.output_token != *collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] invalid swap path"));
        }

        let mut position = match position {
            Some(position) => {
                if position.collateral_token != *collateral_or_swap_out_token {
                    return Err(crate::Error::custom("[sim] collateral token mismatched"));
                }
                PositionModel::new(market, position.clone())?
            }
            None => market.into_empty_position(params.is_long, *collateral_or_swap_out_token)?,
        };

        let report = position
            .increase(
                prices,
                swap_output.amount,
                params.size,
                params.acceptable_price,
            )?
            .execute()?;

        Ok(OrderSimulationOutput::Increase {
            swaps: swap_output.reports,
            report: Box::new(report),
            position,
        })
    }

    fn decrease(self) -> crate::Result<OrderSimulationOutput> {
        let (market, mut prices) = self.market_model_with_prices()?;

        let Self {
            kind,
            graph,
            params,
            collateral_or_swap_out_token,
            position,
            swap_path,
            receive_token,
            ..
        } = self;

        if matches!(
            kind,
            CreateOrderKind::LimitDecrease | CreateOrderKind::StopLossDecrease
        ) {
            let Some(trigger_price) = params.trigger_price else {
                return Err(crate::Error::custom("[sim] trigger price is required"));
            };
            let price = Price {
                min: trigger_price,
                max: trigger_price,
            };
            // NOTE: Collateral token price update not supported yet; may be in future.
            prices.index_token_price = price;
        }

        let Some(position) = position else {
            return Err(crate::Error::custom(
                "[sim] position must be provided for decrease order",
            ));
        };
        if position.collateral_token != *collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] collateral token mismatched"));
        }
        let mut position = PositionModel::new(market, position.clone())?;

        let report = position
            .decrease(
                prices,
                params.size,
                params.acceptable_price,
                params.amount,
                DecreasePositionFlags {
                    is_insolvent_close_allowed: false,
                    is_liquidation_order: false,
                    is_cap_size_delta_usd_allowed: false,
                },
            )?
            .set_swap(
                params
                    .decrease_position_swap_type
                    .map(Into::into)
                    .unwrap_or_default(),
            )
            .execute()?;

        let swaps = if !report.output_amount().is_zero() {
            let source_token = collateral_or_swap_out_token;
            let swap_output =
                graph.swap_along_path(swap_path, source_token, *report.output_amount())?;
            let receive_token = receive_token.unwrap_or(collateral_or_swap_out_token);
            if swap_output.output_token != *receive_token {
                return Err(crate::Error::custom(format!(
                    "[sim] invalid swap path: output_token={}, receive_token={receive_token}",
                    swap_output.output_token
                )));
            }
            swap_output.reports
        } else {
            vec![]
        };

        Ok(OrderSimulationOutput::Decrease {
            swaps,
            report,
            position,
        })
    }

    fn swap(self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let Self {
            kind,
            graph,
            params,
            collateral_or_swap_out_token,
            swap_path,
            pay_token,
            ..
        } = self;

        let swap_in = *pay_token.unwrap_or(collateral_or_swap_out_token);
        let swap_out = *collateral_or_swap_out_token;
        let swap_in_amount = params.amount;
        let swap_out_amount = params.min_output;
        let is_limit_swap = matches!(kind, CreateOrderKind::LimitSwap);

        let mut price_map = HashMap::<_, _>::default();
        if is_limit_swap {
            let swap_in_price = graph.get_price(&swap_in).ok_or_else(|| {
                crate::Error::custom(format!("[sim] price for {swap_in} is not ready"))
            })?;
            let swap_out_price = graph.get_price(&swap_out).ok_or_else(|| {
                crate::Error::custom(format!("[sim] price for {swap_out} is not ready"))
            })?;
            if options.prefer_swap_in_token_update {
                let swap_in_price = swap_out_amount
                    .checked_mul_div_ceil(&swap_out_price.max, &swap_in_amount)
                    .ok_or_else(|| {
                        crate::Error::custom("failed to calculate trigger price for swap in token")
                    })?;
                price_map.insert(swap_in, swap_in_price);
            } else {
                let swap_out_price = swap_in_amount
                    .checked_mul_div_ceil(&swap_in_price.min, &swap_out_amount)
                    .ok_or_else(|| {
                        crate::Error::custom("failed to calculate trigger price for swap in token")
                    })?;
                price_map.insert(swap_out, swap_out_price);
            }
        }

        let swap_output = graph.swap_along_path_with_price_updater(
            swap_path,
            &swap_in,
            params.amount,
            |meta, prices| {
                if !is_limit_swap {
                    return Ok(());
                }
                if let Some(price) = price_map.get(&meta.long_token_mint) {
                    prices.long_token_price.max = *price;
                    prices.long_token_price.min = *price;
                }
                if let Some(price) = price_map.get(&meta.short_token_mint) {
                    prices.short_token_price.max = *price;
                    prices.short_token_price.min = *price;
                }
                Ok(())
            },
        )?;
        if swap_output.output_token != *collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] invalid swap path"));
        }

        Ok(OrderSimulationOutput::Swap(swap_output))
    }
}
