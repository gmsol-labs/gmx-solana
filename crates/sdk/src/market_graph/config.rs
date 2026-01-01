use gmsol_model::price::Prices;
use gmsol_programs::model::{MarketModel, VirtualInventoryModel};
use solana_sdk::pubkey::Pubkey;

use super::{estimation::SwapEstimationParams, SwapEstimation};

/// Config for [`MarketGraph`](super::MarketGraph).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "js", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "js", tsify(from_wasm_abi))]
pub struct MarketGraphConfig {
    /// Estimation Params for swap.
    pub swap_estimation_params: SwapEstimationParams,
    /// Max steps.
    pub max_steps: usize,
}

const DEFAULT_MAX_STEPS: usize = 5;

impl Default for MarketGraphConfig {
    fn default() -> Self {
        Self {
            swap_estimation_params: Default::default(),
            max_steps: DEFAULT_MAX_STEPS,
        }
    }
}

impl MarketGraphConfig {
    pub(super) fn estimate(
        &self,
        market: &MarketModel,
        is_from_long_side: bool,
        prices: Option<Prices<u128>>,
        vi_for_swaps: Option<(Pubkey, &VirtualInventoryModel)>,
    ) -> Option<SwapEstimation> {
        self.swap_estimation_params
            .estimate(market, is_from_long_side, prices, vi_for_swaps)
    }
}
