use std::collections::BTreeMap;

use gmsol_model::{
    action::{swap::SwapReport, withdraw::WithdrawReport},
    LiquidityMarketMutExt, MarketAction,
};
use gmsol_programs::{gmsol_store::types::CreateWithdrawalParams, model::VirtualInventoryModel};
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use super::{SimulationOptions, Simulator};

/// Withdrawal simulation output.
#[derive(Debug)]
pub struct WithdrawalSimulationOutput {
    pub(crate) report: Box<WithdrawReport<u128>>,
    pub(crate) long_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) short_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) long_output_amount: u128,
    pub(crate) short_output_amount: u128,
}

impl WithdrawalSimulationOutput {
    /// Returns long swap reports.
    pub fn long_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.long_swaps
    }

    /// Returns short swap reports.
    pub fn short_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.short_swaps
    }

    /// Returns withdrawal report.
    pub fn report(&self) -> &WithdrawReport<u128> {
        &self.report
    }

    /// Returns long output amount.
    pub fn long_output_amount(&self) -> u128 {
        self.long_output_amount
    }

    /// Returns short output amount.
    pub fn short_output_amount(&self) -> u128 {
        self.short_output_amount
    }
}

/// Withdrawal execution simulation.
#[derive(Debug, TypedBuilder)]
pub struct WithdrawalSimulation<'a> {
    simulator: &'a mut Simulator,
    params: &'a CreateWithdrawalParams,
    market_token: &'a Pubkey,
    #[builder(default)]
    long_receive_token: Option<&'a Pubkey>,
    #[builder(default)]
    long_swap_path: &'a [Pubkey],
    #[builder(default)]
    short_receive_token: Option<&'a Pubkey>,
    #[builder(default)]
    short_swap_path: &'a [Pubkey],
}

impl WithdrawalSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        options: SimulationOptions,
    ) -> crate::Result<WithdrawalSimulationOutput> {
        let Self {
            simulator,
            params,
            market_token,
            long_receive_token,
            long_swap_path,
            short_receive_token,
            short_swap_path,
        } = self;

        if params.market_token_amount == 0 {
            return Err(crate::Error::custom("[sim] empty withdrawal"));
        }

        let (market, prices) = simulator.get_market_with_prices_mut(market_token)?;
        let meta = &market.meta;
        let long_token = meta.long_token_mint;
        let short_token = meta.short_token_mint;
        let long_receive_token = long_receive_token.copied().unwrap_or(long_token);
        let short_receive_token = short_receive_token.copied().unwrap_or(short_token);

        // Execute withdrawal.
        let report = if options.disable_vis {
            market.with_vis_disabled(|market| {
                market
                    .withdraw(u128::from(params.market_token_amount), prices)?
                    .execute()
            })?
        } else {
            let mut vi_map: BTreeMap<Pubkey, VirtualInventoryModel> =
                simulator.vis().map(|(k, v)| (*k, v.clone())).collect();
            market.with_vi_models(&mut vi_map, |market| {
                market
                    .withdraw(u128::from(params.market_token_amount), prices)?
                    .execute()
            })?
        };

        let (long_amount, short_amount) =
            (*report.long_token_output(), *report.short_token_output());

        // Execute swap for long side.
        let (long_swaps, long_output_amount) = if long_amount == 0 {
            (vec![], 0)
        } else {
            let swap_output = simulator.swap_along_path_with_options(
                long_swap_path,
                &long_token,
                long_amount,
                options.clone(),
            )?;

            if swap_output.output_token != long_receive_token {
                return Err(crate::Error::custom("[sim] invalid long swap path"));
            }

            (swap_output.reports, swap_output.amount)
        };

        let min_receive_amount = params.min_long_token_amount;
        if long_output_amount < u128::from(min_receive_amount) {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient long output amount: {long_output_amount} < {min_receive_amount}",
            )));
        }

        // Execute swap for short side.
        let (short_swaps, short_output_amount) = if short_amount == 0 {
            (vec![], 0)
        } else {
            let swap_output = simulator.swap_along_path_with_options(
                short_swap_path,
                &short_token,
                short_amount,
                options.clone(),
            )?;

            if swap_output.output_token != short_receive_token {
                return Err(crate::Error::custom("[sim] invalid short swap path"));
            }

            (swap_output.reports, swap_output.amount)
        };

        let min_receive_amount = params.min_short_token_amount;
        if short_output_amount < u128::from(min_receive_amount) {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient short output amount: {short_output_amount} < {min_receive_amount}",
            )));
        }

        Ok(WithdrawalSimulationOutput {
            report: Box::new(report),
            long_swaps,
            short_swaps,
            long_output_amount,
            short_output_amount,
        })
    }
}
