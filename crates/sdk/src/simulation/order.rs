use std::sync::Arc;

use gmsol_model::{
    action::{
        decrease_position::{DecreasePositionFlags, DecreasePositionReport},
        increase_position::IncreasePositionReport,
        swap::SwapReport,
    },
    num::MulDiv,
    price::Price,
    utils::apply_factor,
    MarketAction, PositionMutExt,
};
use gmsol_programs::{
    constants::{MARKET_DECIMALS, MARKET_USD_UNIT},
    gmsol_store::accounts::Position,
    model::{MarketModel, PositionModel},
};
use rust_decimal::prelude::Zero;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::builders::order::{CreateOrderKind, CreateOrderParams};

use super::simulator::{SimulationOptions, Simulator, SwapOutput};

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
#[derive(Debug, TypedBuilder)]
pub struct OrderSimulation<'a> {
    simulator: &'a mut Simulator,
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

/// Options for prices update.
#[derive(Debug, Default, Clone)]
pub struct UpdatePriceOptions {
    /// Whether to prefer swap in token update.
    pub prefer_swap_in_token_update: bool,
    /// Allowed slippage for limit swap price.
    pub limit_swap_slippage: Option<u128>,
}

impl OrderSimulation<'_> {
    /// Execute the simulation with the given options.
    pub fn execute_with_options(
        self,
        options: SimulationOptions,
    ) -> crate::Result<OrderSimulationOutput> {
        match self.kind {
            CreateOrderKind::MarketIncrease | CreateOrderKind::LimitIncrease => {
                self.increase(options)
            }
            CreateOrderKind::MarketDecrease
            | CreateOrderKind::LimitDecrease
            | CreateOrderKind::StopLossDecrease => self.decrease(options),
            CreateOrderKind::MarketSwap | CreateOrderKind::LimitSwap => self.swap(options),
        }
    }

    fn get_market(&self) -> crate::Result<&MarketModel> {
        let market_token = &self.params.market_token;
        self.simulator.get_market(market_token).ok_or_else(|| {
            crate::Error::custom(format!(
                "[sim] market `{market_token}` not found in the simulator"
            ))
        })
    }

    /// Update the prices in the simulator to execute limit orders.
    pub fn update_prices(self, options: UpdatePriceOptions) -> crate::Result<Self> {
        const DEFAULT_LIMIT_SWAP_SLIPPAGE: u128 = MARKET_USD_UNIT * 5 / 1000;

        match self.kind {
            CreateOrderKind::LimitIncrease
            | CreateOrderKind::LimitDecrease
            | CreateOrderKind::StopLossDecrease => {
                let Some(trigger_price) = self.params.trigger_price else {
                    return Err(crate::Error::custom("[sim] trigger price is required"));
                };
                let token = self.get_market()?.meta.index_token_mint;
                let price = Price {
                    min: trigger_price,
                    max: trigger_price,
                };
                // NOTE: Collateral token price update not supported yet; may be in future.
                self.simulator.insert_price(&token, Arc::new(price))?;
            }
            CreateOrderKind::LimitSwap => {
                let swap_in = *self.pay_token.unwrap_or(self.collateral_or_swap_out_token);
                let swap_out = *self.collateral_or_swap_out_token;
                let swap_in_amount = self.params.amount;
                let swap_out_amount = self.params.min_output;
                let swap_in_price = self.simulator.get_price(&swap_in).ok_or_else(|| {
                    crate::Error::custom(format!("[sim] price for {swap_in} is not ready"))
                })?;
                let swap_out_price = self.simulator.get_price(&swap_out).ok_or_else(|| {
                    crate::Error::custom(format!("[sim] price for {swap_out} is not ready"))
                })?;
                let slippage = options
                    .limit_swap_slippage
                    .unwrap_or(DEFAULT_LIMIT_SWAP_SLIPPAGE);
                if options.prefer_swap_in_token_update {
                    let mut swap_in_price = swap_out_amount
                        .checked_mul_div_ceil(&swap_out_price.max, &swap_in_amount)
                        .ok_or_else(|| {
                            crate::Error::custom(
                                "failed to calculate trigger price for swap in token",
                            )
                        })?;
                    let factor = MARKET_USD_UNIT.checked_add(slippage).ok_or_else(|| {
                        crate::Error::custom(
                            "[sim] failed to calculate factor for applying slippage",
                        )
                    })?;
                    swap_in_price = apply_factor::<_, { MARKET_DECIMALS }>(&swap_in_price, &factor)
                        .ok_or_else(|| {
                            crate::Error::custom("[sim] failed to apply slippage to swap in price")
                        })?;
                    self.simulator.insert_price(
                        &swap_in,
                        Arc::new(Price {
                            min: swap_in_price,
                            max: swap_in_price,
                        }),
                    )?;
                } else {
                    let factor = MARKET_USD_UNIT.checked_sub(slippage).ok_or_else(|| {
                        crate::Error::custom(
                            "[sim] failed to calculate factor for applying slippage",
                        )
                    })?;
                    let mut swap_out_price = swap_in_amount
                        .checked_mul_div_ceil(&swap_in_price.min, &swap_out_amount)
                        .ok_or_else(|| {
                            crate::Error::custom(
                                "failed to calculate trigger price for swap out token",
                            )
                        })?;
                    swap_out_price =
                        apply_factor::<_, { MARKET_DECIMALS }>(&swap_out_price, &factor)
                            .ok_or_else(|| {
                                crate::Error::custom(
                                    "[sim] failed to apply slippage to swap out price",
                                )
                            })?;
                    self.simulator.insert_price(
                        &swap_out,
                        Arc::new(Price {
                            min: swap_out_price,
                            max: swap_out_price,
                        }),
                    )?;
                }
            }
            _ => {}
        }
        Ok(self)
    }

    fn increase(self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let Self {
            kind,
            simulator,
            params,
            collateral_or_swap_out_token,
            position,
            swap_path,
            pay_token,
            ..
        } = self;

        let prices = simulator.get_prices_for_market(&params.market_token)?;

        if matches!(kind, CreateOrderKind::LimitIncrease) && !options.skip_limit_price_validation {
            let Some(trigger_price) = params.trigger_price else {
                return Err(crate::Error::custom("[sim] trigger price is required"));
            };

            // Validate with trigger price.
            let index_price = &prices.index_token_price;
            if params.is_long {
                let price = index_price.pick_price(true);
                if *price > trigger_price {
                    return Err(crate::Error::custom(format!(
                        "[sim] index price must be <= trigger price for a increase-long order, but {price} > {trigger_price}."
                    )));
                }
            } else {
                let price = index_price.pick_price(false);
                if *price < trigger_price {
                    return Err(crate::Error::custom(format!(
                        "[sim] index price must be >= trigger price for a increase-short order, but {price} < {trigger_price}."
                    )));
                }
            }
        }

        let source_token = pay_token.unwrap_or(collateral_or_swap_out_token);
        let swap_output = simulator.swap_along_path(swap_path, source_token, params.amount)?;
        if swap_output.output_token() != collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] invalid swap path"));
        }

        let storage = simulator
            .get_market_mut(&params.market_token)
            .expect("market storage must exist");

        let mut market = storage.clone();

        let mut position = match position {
            Some(position) => {
                if position.collateral_token != *collateral_or_swap_out_token {
                    return Err(crate::Error::custom("[sim] collateral token mismatched"));
                }
                market.with_vis_disabled(|market| {
                    PositionModel::new(market.clone(), position.clone())
                })?
            }
            None => market.with_vis_disabled(|market| {
                market
                    .clone()
                    .into_empty_position(params.is_long, *collateral_or_swap_out_token)
            })?,
        };

        let report = position
            .increase(
                prices,
                swap_output.amount(),
                params.size,
                params.acceptable_price,
            )?
            .execute()?;

        *storage = position.market_model().clone();

        Ok(OrderSimulationOutput::Increase {
            swaps: swap_output.reports,
            report: Box::new(report),
            position,
        })
    }

    fn decrease(self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let Self {
            kind,
            simulator,
            params,
            collateral_or_swap_out_token,
            position,
            swap_path,
            receive_token,
            ..
        } = self;

        let prices = simulator.get_prices_for_market(&params.market_token)?;

        // Validate with trigger price.
        if !options.skip_limit_price_validation {
            let index_price = &prices.index_token_price;
            let is_long = params.is_long;
            match kind {
                CreateOrderKind::LimitDecrease => {
                    let Some(trigger_price) = params.trigger_price else {
                        return Err(crate::Error::custom("[sim] trigger price is required"));
                    };
                    if is_long {
                        let price = index_price.pick_price(false);
                        if *price < trigger_price {
                            return Err(crate::Error::custom(format!(
                            "[sim] index price must be >= trigger price for a limit-decrease-long order, but {price} < {trigger_price}."
                        )));
                        }
                    } else {
                        let price = index_price.pick_price(true);
                        if *price > trigger_price {
                            return Err(crate::Error::custom(format!(
                            "[sim] index price must be <= trigger price for a limit-decrease-short order, but {price} > {trigger_price}."
                        )));
                        }
                    }
                }
                CreateOrderKind::StopLossDecrease => {
                    let Some(trigger_price) = params.trigger_price else {
                        return Err(crate::Error::custom("[sim] trigger price is required"));
                    };
                    if is_long {
                        let price = index_price.pick_price(false);
                        if *price > trigger_price {
                            return Err(crate::Error::custom(format!(
                            "[sim] index price must be <= trigger price for a stop-loss-decrease-long order, but {price} > {trigger_price}."
                        )));
                        }
                    } else {
                        let price = index_price.pick_price(true);
                        if *price < trigger_price {
                            return Err(crate::Error::custom(format!(
                            "[sim] index price must be >= trigger price for a stop-loss-decrease-short order, but {price} < {trigger_price}."
                        )));
                        }
                    }
                }
                _ => {}
            }
        }

        let Some(position) = position else {
            return Err(crate::Error::custom(
                "[sim] position must be provided for decrease order",
            ));
        };
        if position.collateral_token != *collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] collateral token mismatched"));
        }

        let storage = simulator
            .get_market_mut(&params.market_token)
            .expect("market storage must exist");

        let mut market = storage.clone();

        let mut position = market
            .with_vis_disabled(|market| PositionModel::new(market.clone(), position.clone()))?;

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

        *storage = position.market_model().clone();

        let swaps = if !report.output_amount().is_zero() {
            let source_token = collateral_or_swap_out_token;
            let swap_output =
                simulator.swap_along_path(swap_path, source_token, *report.output_amount())?;
            let receive_token = receive_token.unwrap_or(collateral_or_swap_out_token);
            if swap_output.output_token() != receive_token {
                return Err(crate::Error::custom(format!(
                    "[sim] invalid swap path: output_token={}, receive_token={receive_token}",
                    swap_output.output_token()
                )));
            }
            // Ensure the market model of the position is in-sync with the simulator's.
            position.set_market_model(
                simulator
                    .get_market(&params.market_token)
                    .expect("market storage must exist"),
            );
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
            simulator,
            params,
            collateral_or_swap_out_token,
            swap_path,
            pay_token,
            ..
        } = self;

        let swap_in = *pay_token.unwrap_or(collateral_or_swap_out_token);

        let swap_output = simulator.swap_along_path(swap_path, &swap_in, params.amount)?;
        if swap_output.output_token() != collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] invalid swap path"));
        }

        if matches!(kind, CreateOrderKind::LimitSwap) && !options.skip_limit_price_validation {
            let output_amount = swap_output.amount();
            let min_output_amount = params.min_output;
            if output_amount < min_output_amount {
                return Err(crate::Error::custom(format!("[sim] the limit swap output is too low, {output_amount} < min_output = {min_output_amount}. Has the limit price been reached?")));
            }
        }

        Ok(OrderSimulationOutput::Swap(swap_output))
    }
}
