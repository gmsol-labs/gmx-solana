use gmsol_model::{
    action::{swap::SwapReport, withdraw::WithdrawReport},
    LiquidityMarketMutExt, MarketAction,
};
use gmsol_programs::gmsol_store::types::CreateGlvWithdrawalParams;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::constants;

use super::{SimulationOptions, Simulator};

/// GLV withdrawal simulation output.
#[derive(Debug)]
pub struct GlvWithdrawalSimulationOutput {
    pub(crate) report: Box<WithdrawReport<u128>>,
    pub(crate) long_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) short_swaps: Vec<SwapReport<u128, i128>>,
    pub(crate) long_output_amount: u128,
    pub(crate) short_output_amount: u128,
}

impl GlvWithdrawalSimulationOutput {
    /// Returns long swap reports.
    pub fn long_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.long_swaps
    }

    /// Returns short swap reports.
    pub fn short_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.short_swaps
    }

    /// Returns withdraw report.
    pub fn withdraw_report(&self) -> &WithdrawReport<u128> {
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

/// GLV withdrawal execution simulation.
#[derive(Debug, TypedBuilder)]
pub struct GlvWithdrawalSimulation<'a> {
    simulator: &'a mut Simulator,
    params: &'a CreateGlvWithdrawalParams,
    glv_token: &'a Pubkey,
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

impl GlvWithdrawalSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        _options: SimulationOptions,
    ) -> crate::Result<GlvWithdrawalSimulationOutput> {
        let Self {
            simulator,
            params,
            glv_token,
            market_token,
            long_receive_token,
            long_swap_path,
            short_receive_token,
            short_swap_path,
        } = self;

        if params.glv_token_amount == 0 {
            return Err(crate::Error::custom("[sim] empty GLV withdrawal"));
        }

        let (market, prices) = simulator.get_market_with_prices(market_token)?;
        let meta = &market.meta;
        let long_token = meta.long_token_mint;
        let short_token = meta.short_token_mint;
        let long_receive_token = long_receive_token.copied().unwrap_or(long_token);
        let short_receive_token = short_receive_token.copied().unwrap_or(short_token);

        // Calculate market token amount to withdrawal.
        let glv_value = simulator.get_glv_value(glv_token, false)?;
        let glv_supply = simulator.get_glv(glv_token).expect("must exist").supply();
        let market_token_value = gmsol_model::utils::market_token_amount_to_usd(
            &u128::from(params.glv_token_amount),
            &glv_value,
            &u128::from(glv_supply),
        )
        .ok_or(crate::Error::custom(
            "[sim] failed to calculate market token value for GLV withdrawal",
        ))?;

        let market_token_amount = gmsol_model::glv::get_market_token_amount_for_glv_value(
            &prices,
            market,
            market_token_value,
            true,
            constants::MARKET_USD_TO_AMOUNT_DIVISOR,
        )?
        .try_into()
        .map_err(|_| crate::Error::custom("[sim] market token amount to withdraw overflow"))?;

        simulator
            .get_glv_mut(glv_token)
            .expect("must exist")
            .withdraw_from_glv(market_token, market_token_amount, params.glv_token_amount)?;

        let market = simulator.get_market_mut(market_token).expect("must exist");

        // Execute withdrawal.
        let report = market
            .withdraw(u128::from(market_token_amount), prices)?
            .execute()?;

        let (long_amount, short_amount) =
            (*report.long_token_output(), *report.short_token_output());

        // Execute swap for long side.
        let (long_swaps, long_output_amount) = if long_amount == 0 {
            (vec![], 0)
        } else {
            let swap_output =
                simulator.swap_along_path(long_swap_path, &long_token, long_amount)?;

            if swap_output.output_token != long_receive_token {
                return Err(crate::Error::custom("[sim] invalid long swap path"));
            }

            (swap_output.reports, swap_output.amount)
        };

        let min_receive_amount = params.min_final_long_token_amount;
        if long_output_amount < u128::from(min_receive_amount) {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient long output amount: {long_output_amount} < {min_receive_amount}",
            )));
        }

        // Execute swap for short side.
        let (short_swaps, short_output_amount) = if short_amount == 0 {
            (vec![], 0)
        } else {
            let swap_output =
                simulator.swap_along_path(short_swap_path, &short_token, short_amount)?;

            if swap_output.output_token != short_receive_token {
                return Err(crate::Error::custom("[sim] invalid short swap path"));
            }

            (swap_output.reports, swap_output.amount)
        };

        let min_receive_amount = params.min_final_short_token_amount;
        if short_output_amount < u128::from(min_receive_amount) {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient short output amount: {short_output_amount} < {min_receive_amount}",
            )));
        }

        Ok(GlvWithdrawalSimulationOutput {
            report: Box::new(report),
            long_swaps,
            short_swaps,
            long_output_amount,
            short_output_amount,
        })
    }
}
