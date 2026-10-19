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
    MarketAction, PositionExt, PositionMutExt,
};
use gmsol_programs::{
    constants::{MARKET_DECIMALS, MARKET_USD_UNIT},
    gmsol_store::accounts::Position,
    model::{MarketModel, PositionModel, VirtualInventoryModel},
};
use rust_decimal::prelude::Zero;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::builders::order::{CreateOrderKind, CreateOrderParams, DecreasePositionSwapType};

use super::{
    builder_fee::{self, BuilderFeeOutcome, TokenReceipt},
    simulator::{SimulationOptions, Simulator, SwapOutput},
};

/// Order simulation output.
///
/// For an order that carries a builder fee, the amounts are those the program
/// produces on execution: see `builder_fee_factor` on the order simulation builder.
#[derive(Debug)]
pub enum OrderSimulationOutput {
    /// Increase output.
    Increase {
        swaps: Vec<SwapReport<u128, i128>>,
        /// The model's report. Its `params().collateral_increment_amount()` is
        /// the **net** collateral the position received, i.e.
        /// `gross_collateral_amount` minus the builder fee, as on chain.
        report: Box<IncreasePositionReport<u128, i128>>,
        position: PositionModel,
        /// The collateral produced by the pay-token swap, before the builder fee.
        gross_collateral_amount: u128,
        /// The builder fee, if the order carries one.
        builder_fee: Option<BuilderFeeOutcome>,
    },
    /// Decrease output.
    Decrease {
        swaps: Vec<SwapReport<u128, i128>>,
        /// The model's report. It holds the **gross** payout: the program does
        /// not net the builder fee out of the transferred output. See
        /// `receipts` for the amounts after the fee.
        report: Box<DecreasePositionReport<u128, i128>>,
        position: PositionModel,
        /// What the owner holds per token after the receive-token swap, with the
        /// builder fee taken out of the final output token.
        ///
        /// The final output token always comes first. A secondary (pnl token)
        /// output in the same token is merged into it, and one in another token
        /// follows as its own entry, omitted when zero.
        receipts: Vec<TokenReceipt>,
        /// The builder fee, if the order carries one.
        builder_fee: Option<BuilderFeeOutcome>,
    },
    /// Swap output.
    Swap(SwapOutput),
}

impl OrderSimulationOutput {
    /// Returns the builder fee, if the order carries one.
    ///
    /// `None` means the order has no builder fee. `Some` with a zero
    /// `paid_amount` means it has one but nothing was collectible.
    pub fn builder_fee(&self) -> Option<&BuilderFeeOutcome> {
        match self {
            Self::Increase { builder_fee, .. } | Self::Decrease { builder_fee, .. } => {
                builder_fee.as_ref()
            }
            Self::Swap(_) => None,
        }
    }

    /// Returns the collateral the position received from an increase, after the
    /// builder fee.
    pub fn net_collateral_amount(&self) -> Option<u128> {
        match self {
            Self::Increase { report, .. } => Some(*report.params().collateral_increment_amount()),
            _ => None,
        }
    }

    /// Returns the per-token receipts of a decrease, after the builder fee.
    pub fn receipts(&self) -> &[TokenReceipt] {
        match self {
            Self::Decrease { receipts, .. } => receipts,
            _ => &[],
        }
    }
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
    /// The builder fee factor checkpointed onto the order. `0` (the default)
    /// means the order has no builder fee, and leaves every result unchanged.
    ///
    /// Models the program's builder fee at the same execution stages:
    ///
    /// - **Increase**: the fee is deducted from the swapped collateral before
    ///   the position increases, and the simulation fails if the collateral
    ///   cannot cover it.
    /// - **Decrease**: the collateral withdrawal is topped up by an estimate of
    ///   the fee, which can change a partial decrease into a full close. The
    ///   fee is then charged on the executed size in the final output token,
    ///   clamped to the outputs that fund its escrow. `CollateralToPnlToken` is
    ///   rejected.
    ///
    /// Swap orders cannot carry a builder fee, so a non-zero factor is rejected.
    ///
    /// Read the factor from the order account (`Order.builder_fee_factor`) for an
    /// order already checkpointed. That is what the program charges, not the
    /// factor the builder advertises now. For an order not yet checkpointed, use
    /// the builder's advertised factor (`UserHeader.builder_fee_factor`), which
    /// is what `set_builder_fee` requires as `expected_factor`. An order that
    /// will not be checkpointed executes with no fee: leave this at `0`.
    ///
    /// When a simulation with a non-zero factor fails, the simulator is restored
    /// to its state before the call, as the failed execution would be on chain.
    #[builder(default)]
    builder_fee_factor: u128,
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
        mut self,
        options: SimulationOptions,
    ) -> crate::Result<OrderSimulationOutput> {
        let is_position_order = !matches!(
            self.kind,
            CreateOrderKind::MarketSwap | CreateOrderKind::LimitSwap
        );
        if self.builder_fee_factor != 0 && !is_position_order {
            return Err(crate::Error::custom(
                "[sim] a builder fee applies to position orders only, but the order is a swap",
            ));
        }

        // An order with a builder fee can fail on a condition the caller should
        // expect (the fee exceeding the swapped collateral). On chain that
        // failure reverts everything, including the swap that came before it, so
        // the simulator is restored here rather than left with the swap applied.
        let snapshot = (self.builder_fee_factor != 0).then(|| self.simulator.clone());

        let result = match self.kind {
            CreateOrderKind::MarketIncrease | CreateOrderKind::LimitIncrease => {
                self.increase(options)
            }
            CreateOrderKind::MarketDecrease
            | CreateOrderKind::LimitDecrease
            | CreateOrderKind::StopLossDecrease => self.decrease(options),
            CreateOrderKind::MarketSwap | CreateOrderKind::LimitSwap => self.swap(options),
        };

        if result.is_err() {
            if let Some(snapshot) = snapshot {
                *self.simulator = snapshot;
            }
        }
        result
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

    fn increase(&mut self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let kind = self.kind;
        let params = self.params;
        let collateral_or_swap_out_token = self.collateral_or_swap_out_token;
        let position = self.position;
        let swap_path = self.swap_path;
        let pay_token = self.pay_token;
        let builder_fee_factor = self.builder_fee_factor;
        let simulator = &mut *self.simulator;

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

        let ((report, builder_fee), position) = with_vi_models_if_some(
            &market_snapshot,
            position,
            vi_ctx,
            params.is_long,
            collateral_or_swap_out_token,
            move |position_model: &mut PositionModel| {
                // The fee is charged in the collateral token, after the pay token
                // has been swapped into it, and is deducted from the collateral
                // before the position increases.
                let (collateral_increment_amount, builder_fee) =
                    builder_fee::charge_on_collateral_increment(
                        swap_amount,
                        params.size,
                        builder_fee_factor,
                        collateral_or_swap_out_token,
                        position_model.collateral_price(&prices),
                    )?;
                let report = position_model
                    .increase(
                        prices,
                        collateral_increment_amount,
                        params.size,
                        params.acceptable_price,
                    )?
                    .execute()?;
                Ok((report, builder_fee))
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
            gross_collateral_amount: swap_amount,
            builder_fee,
        })
    }

    fn decrease(&mut self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let kind = self.kind;
        let params = self.params;
        let collateral_or_swap_out_token = self.collateral_or_swap_out_token;
        let position = self.position;
        let swap_path = self.swap_path;
        let receive_token = self.receive_token;
        let builder_fee_factor = self.builder_fee_factor;
        let simulator = &mut *self.simulator;

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

        // The final output token is where the builder fee is paid from. Resolve
        // what it needs now, before the market is touched, so that these
        // failures leave the simulator as it was.
        let final_output_token = *receive_token.unwrap_or(collateral_or_swap_out_token);
        let final_output_price = if builder_fee_factor != 0 {
            if matches!(
                params.decrease_position_swap_type,
                Some(DecreasePositionSwapType::CollateralToPnlToken)
            ) {
                // Rejected by the program for any non-zero factor, whatever the
                // fee estimate rounds to.
                return Err(crate::Error::custom(
                    "[sim] BuilderFeeSwapTypeNotAllowed: `CollateralToPnlToken` cannot be used \
                     with a non-zero builder fee factor",
                ));
            }
            Some(simulator.get_price(&final_output_token).ok_or_else(|| {
                crate::Error::custom(format!("[sim] price for {final_output_token} is not ready"))
            })?)
        } else {
            None
        };

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
                // Fold an estimate of the builder fee into the withdrawal, so
                // that it is still funded when the position closes at a loss.
                // This is what can turn a partial decrease into a full close.
                let collateral_withdrawal_amount = builder_fee::withdrawal_with_estimate(
                    params.amount,
                    params.size,
                    builder_fee_factor,
                    position_model.collateral_price(&prices),
                    params.decrease_position_swap_type,
                )?;
                let report = position_model
                    .decrease(
                        prices,
                        params.size,
                        params.acceptable_price,
                        collateral_withdrawal_amount,
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

        let mut final_output_amount = 0;
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
            final_output_amount = swap_output.amount();
            swap_output.reports
        } else {
            vec![]
        };

        // The fee is charged in the final output token, after the receive-token
        // swap, on the size that was actually executed (a decrease may have been
        // enlarged into a full close) and at that token's price. It is computed
        // and clamped but never netted from the report: the program transfers
        // the whole output to the escrow and settlement moves the fee out of it.
        //
        // The secondary output is the pnl token, which the report knows as a side
        // of the market.
        let secondary_output_token = if report.is_secondary_output_token_long() {
            market_snapshot.meta.long_token_mint
        } else {
            market_snapshot.meta.short_token_mint
        };
        let secondary_output_amount = *report.secondary_output_amount();
        let builder_fee = match final_output_price {
            Some(final_output_price) => {
                let payable_amount = builder_fee::compute_amount(
                    *report.size_delta_usd(),
                    builder_fee_factor,
                    &final_output_price,
                )?;
                let paid_amount = builder_fee::clamp_to_outputs(
                    payable_amount,
                    &final_output_token,
                    final_output_amount,
                    &secondary_output_token,
                    secondary_output_amount,
                )?;
                Some(BuilderFeeOutcome {
                    factor: builder_fee_factor,
                    token: final_output_token,
                    payable_amount,
                    paid_amount,
                })
            }
            None => None,
        };
        let receipts = builder_fee::build_receipts(
            &final_output_token,
            final_output_amount,
            &secondary_output_token,
            secondary_output_amount,
            builder_fee.as_ref().map_or(0, |fee| fee.paid_amount),
        )?;

        Ok(OrderSimulationOutput::Decrease {
            swaps,
            report,
            position,
            receipts,
            builder_fee,
        })
    }

    fn swap(&mut self, options: SimulationOptions) -> crate::Result<OrderSimulationOutput> {
        let kind = self.kind;
        let params = self.params;
        let collateral_or_swap_out_token = self.collateral_or_swap_out_token;
        let swap_path = self.swap_path;
        let pay_token = self.pay_token;
        let simulator = &mut *self.simulator;

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
    let (output, mut position) = market.with_vis_if(vi_map, |market_in_scope| {
        let mut position =
            make_position_model(market_in_scope, position, is_long, collateral_token)?;
        let output = f(&mut position)?;
        *market_in_scope = position.market_model().clone();
        crate::Result::Ok((output, position))
    })?;
    position.set_market_model(&market);
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
