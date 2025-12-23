use std::collections::BTreeMap;

use gmsol_model::{price::Prices, utils::div_to_factor, MarketAction, SwapMarketMutExt};
use gmsol_programs::model::{MarketModel, VirtualInventoryModel};
use rust_decimal::{Decimal, MathematicalOps};

use crate::constants;

#[derive(Debug, Clone)]
pub(super) struct SwapEstimation {
    pub(super) ln_exchange_rate: Decimal,
}

/// Estimation Parameters for Swap.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "js", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "js", tsify(from_wasm_abi, into_wasm_abi))]
pub struct SwapEstimationParams {
    /// Value.
    pub value: u128,
    /// Base cost.
    pub base_cost: u128,
}

const DEFAULT_VALUE: u128 = 1_000 * constants::MARKET_USD_UNIT;
const DEFAULT_BASE_COST: u128 = 2 * constants::MARKET_USD_UNIT / 100;

impl Default for SwapEstimationParams {
    fn default() -> Self {
        Self {
            value: DEFAULT_VALUE,
            base_cost: DEFAULT_BASE_COST,
        }
    }
}

impl SwapEstimationParams {
    pub(super) fn estimate(
        &self,
        market: &MarketModel,
        is_from_long_side: bool,
        prices: Option<Prices<u128>>,
        vi_for_swaps: Option<&VirtualInventoryModel>,
    ) -> Option<SwapEstimation> {
        if self.value == 0 {
            #[cfg(tracing)]
            {
                tracing::trace!("estimation failed with zero input value");
            }
            return None;
        }
        let prices = prices?;
        let mut market = market.clone();
        let token_in_amount = self
            .value
            .checked_div(prices.collateral_token_price(is_from_long_side).min)?;
        let swap = if let Some(vi) = vi_for_swaps {
            let mut vi_map = BTreeMap::new();
            if let Some(vi_addr) = market.meta.virtual_inventory_for_swaps {
                vi_map.insert(vi_addr, vi.clone());
            }
            market.with_vi_models(&mut vi_map, |market| {
                market
                    .swap(is_from_long_side, token_in_amount, prices)
                    .inspect_err(|err| {
                        #[cfg(tracing)]
                        {
                            tracing::trace!("estimation failed when creating swap: {err}");
                        }
                        _ = err;
                    })
                    .ok()
                    .and_then(|action| {
                        action
                            .execute()
                            .inspect_err(|err| {
                                #[cfg(tracing)]
                                {
                                    tracing::trace!("estimation failed when executing swap: {err}");
                                }
                                _ = err;
                            })
                            .ok()
                    })
            })?
        } else {
            market.with_vis_disabled(|market| {
                market
                    .swap(is_from_long_side, token_in_amount, prices)
                    .inspect_err(|err| {
                        #[cfg(tracing)]
                        {
                            tracing::trace!("estimation failed when creating swap: {err}");
                        }
                        _ = err;
                    })
                    .ok()
                    .and_then(|action| {
                        action
                            .execute()
                            .inspect_err(|err| {
                                #[cfg(tracing)]
                                {
                                    tracing::trace!("estimation failed when executing swap: {err}");
                                }
                                _ = err;
                            })
                            .ok()
                    })
            })?
        };
        let token_out_value = swap
            .token_out_amount()
            .checked_mul(prices.collateral_token_price(!is_from_long_side).max)?;
        if token_out_value <= self.base_cost {
            #[cfg(tracing)]
            {
                tracing::trace!("estimation failed with zero output value");
            }
            return None;
        }
        let token_out_value = token_out_value.abs_diff(self.base_cost);
        let exchange_rate = div_to_factor::<_, { crate::constants::MARKET_DECIMALS }>(
            &token_out_value,
            &self.value,
            false,
        )?;
        let exchange_rate = crate::utils::fixed::unsigned_value_to_decimal(exchange_rate);
        let ln_exchange_rate = exchange_rate.checked_ln()?;
        Some(SwapEstimation { ln_exchange_rate })
    }
}
