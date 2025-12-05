use gmsol_model::{
    action::{deposit::DepositReport, swap::SwapReport},
    LiquidityMarketMutExt, MarketAction,
};
use gmsol_programs::gmsol_store::types::CreateDepositParams;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use super::{SimulationOptions, Simulator};

/// Deposit simulation output.
#[derive(Debug)]
pub struct DepositSimulationOutput {
    pub(crate) long_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) short_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) report: Box<DepositReport<u128, i128>>,
}

impl DepositSimulationOutput {
    /// Returns long swap reports.
    pub fn long_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.long_swaps
    }

    /// Returns short swap reports.
    pub fn short_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.short_swaps
    }

    /// Returns deposit report.
    pub fn report(&self) -> &DepositReport<u128, i128> {
        &self.report
    }
}

/// Deposit execution simulation.
#[derive(Debug, TypedBuilder)]
pub struct DepositSimulation<'a> {
    simulator: &'a mut Simulator,
    params: &'a CreateDepositParams,
    market_token: &'a Pubkey,
    #[builder(default)]
    long_pay_token: Option<&'a Pubkey>,
    #[builder(default)]
    long_swap_path: &'a [Pubkey],
    #[builder(default)]
    short_pay_token: Option<&'a Pubkey>,
    #[builder(default)]
    short_swap_path: &'a [Pubkey],
}

impl DepositSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        options: SimulationOptions,
    ) -> crate::Result<DepositSimulationOutput> {
        let Self {
            simulator,
            params,
            market_token,
            long_pay_token,
            long_swap_path,
            short_pay_token,
            short_swap_path,
        } = self;

        if params.initial_long_token_amount == 0 && params.initial_short_token_amount == 0 {
            return Err(crate::Error::custom("[sim] empty deposit"));
        }

        let (prices, meta) = simulator.get_prices_and_meta_for_market(market_token)?;

        // Execute swaps.
        let long_token = meta.long_token_mint;
        let short_token = meta.short_token_mint;
        let long_pay_token = long_pay_token.copied().unwrap_or(long_token);
        let short_pay_token = short_pay_token.copied().unwrap_or(short_token);

        let long_swap_output = simulator.swap_along_path_with_options(
            long_swap_path,
            &long_pay_token,
            params.initial_long_token_amount.into(),
            options.clone(),
        )?;
        if long_swap_output.output_token != long_token {
            return Err(crate::Error::custom("[sim] invalid long swap path"));
        }

        let short_swap_output = simulator.swap_along_path_with_options(
            short_swap_path,
            &short_pay_token,
            params.initial_short_token_amount.into(),
            options.clone(),
        )?;
        if short_swap_output.output_token != short_token {
            return Err(crate::Error::custom("[sim] invalid short swap path"));
        }

        // Execute deposit.
        let report = if options.disable_vis {
            let market = simulator
                .get_market_mut(market_token)
                .expect("market storage must exist");
            market.with_vis_disabled(|market| {
                market
                    .deposit(long_swap_output.amount, short_swap_output.amount, prices)?
                    .execute()
            })?
        } else {
            let (market, vi_map) = simulator.get_market_and_vis_mut(market_token)?;
            market.with_vi_models(vi_map, |market| {
                market
                    .deposit(long_swap_output.amount, short_swap_output.amount, prices)?
                    .execute()
            })?
        };

        let minted = report.minted();
        let min_market_token_amount = u128::from(params.min_market_token_amount);
        if *minted < min_market_token_amount {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient output amount: {minted} < {min_market_token_amount}",
            )));
        }

        Ok(DepositSimulationOutput {
            long_swaps: long_swap_output.reports,
            short_swaps: short_swap_output.reports,
            report: Box::new(report),
        })
    }
}
