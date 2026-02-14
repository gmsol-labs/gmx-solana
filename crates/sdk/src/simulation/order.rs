use std::{collections::BTreeMap, sync::Arc};

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
    model::{MarketModel, PositionModel, VirtualInventoryModel},
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
        let swap_output = simulator.swap_along_path_with_options(
            swap_path,
            source_token,
            params.amount,
            options.clone(),
        )?;
        if swap_output.output_token() != collateral_or_swap_out_token {
            return Err(crate::Error::custom("[sim] invalid swap path"));
        }

        // Execute the increase against a cloned market model, while VI state
        // is managed exclusively via the simulator's global VI map.
        let market_snapshot = {
            let market = simulator.get_market(&params.market_token).ok_or_else(|| {
                crate::Error::custom(format!(
                    "[sim] market `{}` not found in the simulator",
                    params.market_token
                ))
            })?;
            market.clone()
        };

        let swap_amount = swap_output.amount();
        let vi_ctx = if options.disable_vis {
            None
        } else {
            Some(simulator.vis_mut())
        };

        let (report, position) = with_vi_models_if_some(
            &market_snapshot,
            position,
            vi_ctx,
            params.is_long,
            collateral_or_swap_out_token,
            move |position_model: &mut PositionModel| {
                let report = position_model
                    .increase(prices, swap_amount, params.size, params.acceptable_price)?
                    .execute()?;
                Ok(report)
            },
        )?;

        // Persist the evolved market model back into the simulator environment.
        {
            let storage = simulator
                .get_market_mut(&params.market_token)
                .expect("market storage must exist");
            *storage = position.market_model().clone();
        }

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

        // Execute the decrease against a cloned market model, while VI state
        // is managed exclusively via the simulator's global VI map.
        let market_snapshot = {
            let market = simulator.get_market(&params.market_token).ok_or_else(|| {
                crate::Error::custom(format!(
                    "[sim] market `{}` not found in the simulator",
                    params.market_token
                ))
            })?;
            market.clone()
        };

        let vi_ctx = if options.disable_vis {
            None
        } else {
            Some(simulator.vis_mut())
        };

        let (report, mut position) = with_vi_models_if_some(
            &market_snapshot,
            Some(position),
            vi_ctx,
            params.is_long,
            collateral_or_swap_out_token,
            move |position_model: &mut PositionModel| {
                let report = position_model
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
                Ok(report)
            },
        )?;

        // Persist the evolved market model back into the simulator environment.
        {
            let storage = simulator
                .get_market_mut(&params.market_token)
                .expect("market storage must exist");
            *storage = position.market_model().clone();
        }

        let swaps = if !report.output_amount().is_zero() {
            let source_token = collateral_or_swap_out_token;
            let swap_output = simulator.swap_along_path_with_options(
                swap_path,
                source_token,
                *report.output_amount(),
                options.clone(),
            )?;
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

        let swap_output = simulator.swap_along_path_with_options(
            swap_path,
            &swap_in,
            params.amount,
            options.clone(),
        )?;
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

fn with_vi_models_if_some<T>(
    market: &MarketModel,
    position: Option<&Arc<Position>>,
    vi_map: Option<&mut BTreeMap<Pubkey, VirtualInventoryModel>>,
    is_long: bool,
    collateral_token: &Pubkey,
    f: impl FnOnce(&mut PositionModel) -> crate::Result<T>,
) -> crate::Result<(T, PositionModel)> {
    let mut market: MarketModel = market.clone();
    let should_update_market = vi_map.is_some();
    let (output, mut position) = market.with_vis_if(vi_map, |market_in_scope| {
        let mut position =
            make_position_model(market_in_scope, position, is_long, collateral_token)?;
        let output = f(&mut position)?;
        *market_in_scope = position.market_model().clone();
        crate::Result::Ok((output, position))
    })?;
    if should_update_market {
        position.set_market_model(&market);
    }
    Ok((output, position))
}

fn make_position_model(
    market: &MarketModel,
    position: Option<&Arc<Position>>,
    is_long: bool,
    collateral_token: &Pubkey,
) -> crate::Result<PositionModel> {
    match position {
        Some(position) => {
            if position.collateral_token != *collateral_token {
                return Err(crate::Error::custom("[sim] collateral token mismatched"));
            }
            Ok(PositionModel::new(market.clone(), position.clone())?)
        }
        None => Ok(market
            .clone()
            .into_empty_position(is_long, *collateral_token)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmsol_model::BaseMarket;
    use gmsol_programs::{bytemuck::Zeroable, gmsol_store::accounts::Market};

    fn create_market_with_vi_address() -> Market {
        let mut market: Market = Zeroable::zeroed();
        market.virtual_inventory_for_positions = Pubkey::new_unique();
        market
    }

    #[test]
    fn test_disable_vis_state_preserved_when_vi_map_is_none() {
        let market = create_market_with_vi_address();
        let market_model = MarketModel::from_parts(Arc::new(market), 1_000_000_000);

        let long_token = market_model.meta.long_token_mint;
        let is_long = true;

        let result = with_vi_models_if_some(
            &market_model,
            None,
            None,
            is_long,
            &long_token,
            |_position_model| Ok(()),
        );

        assert!(result.is_ok());
        let (_output, position) = result.unwrap();

        assert!(
            position.market_model().is_vis_disabled_for_test(),
            "When vi_map = None, returned position's disable_vis should be true"
        );
    }

    #[test]
    fn test_vi_validation_fails_when_disable_vis_is_wrong() {
        let market = create_market_with_vi_address();
        let market_model = MarketModel::from_parts(Arc::new(market), 1_000_000_000);

        let result = market_model.virtual_inventory_for_positions_pool();

        match result {
            Err(err) => {
                assert!(
                    err.to_string().contains(
                        "virtual inventory for positions should be present but is missing"
                    ),
                    "Unexpected error: {err}"
                );
            }
            Ok(_) => {
                panic!("Should fail when has VI address but no VI data and disable_vis = false");
            }
        }
    }

    #[test]
    fn test_vi_validation_skipped_when_vis_disabled() {
        let market = create_market_with_vi_address();
        let mut market_model = MarketModel::from_parts(Arc::new(market), 1_000_000_000);

        let is_disabled_in_closure =
            market_model.with_vis_disabled(|m| m.is_vis_disabled_for_test());

        assert!(
            is_disabled_in_closure,
            "disable_vis should be true inside closure"
        );
        assert!(
            !market_model.is_vis_disabled_for_test(),
            "disable_vis should be restored after closure"
        );
    }
}
