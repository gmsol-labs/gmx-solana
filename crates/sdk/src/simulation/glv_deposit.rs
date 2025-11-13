use gmsol_model::{
    action::{deposit::DepositReport, swap::SwapReport},
    BaseMarketExt, PnlFactorKind,
};
use gmsol_programs::gmsol_store::types::{CreateDepositParams, CreateGlvDepositParams};
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::simulation::deposit::DepositSimulation;

use super::{deposit::DepositSimulationOutput, SimulationOptions, Simulator};

/// GLV deposit simulation output.
#[derive(Debug)]
pub struct GlvDepositSimulationOutput {
    long_swaps: Vec<SwapReport<u128, i128>>,
    short_swaps: Vec<SwapReport<u128, i128>>,
    deposit_report: Option<Box<DepositReport<u128, i128>>>,
    output_amount: u64,
}

impl GlvDepositSimulationOutput {
    /// Returns long swap reports.
    pub fn long_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.long_swaps
    }

    /// Returns short swap reports.
    pub fn short_swaps(&self) -> &[SwapReport<u128, i128>] {
        &self.short_swaps
    }

    /// Returns deposit report.
    pub fn deposit_report(&self) -> Option<&DepositReport<u128, i128>> {
        self.deposit_report.as_deref()
    }

    /// Returns the output amount.
    pub fn output_amount(&self) -> u64 {
        self.output_amount
    }
}

/// GLV deposit execution simulation.
#[derive(Debug, TypedBuilder)]
pub struct GlvDepositSimulation<'a> {
    simulator: &'a mut Simulator,
    params: &'a CreateGlvDepositParams,
    glv_token: &'a Pubkey,
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

impl GlvDepositSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        options: SimulationOptions,
    ) -> crate::Result<GlvDepositSimulationOutput> {
        let Self {
            simulator,
            params,
            glv_token,
            market_token,
            long_pay_token,
            long_swap_path,
            short_pay_token,
            short_swap_path,
        } = self;

        let is_deposit_empty =
            params.initial_long_token_amount == 0 && params.initial_short_token_amount == 0;

        // Validate Max PNL (to stay consistent with the contract code)
        if params.market_token_amount != 0 {
            let (market, prices) = simulator.get_market_with_prices(market_token)?;
            market.validate_max_pnl(
                &prices,
                PnlFactorKind::MaxAfterWithdrawal,
                PnlFactorKind::MaxAfterWithdrawal,
            )?;
        }

        // Execute normal deposit.
        let deposit_output = (!is_deposit_empty)
            .then(|| {
                DepositSimulation::builder()
                    .simulator(simulator)
                    .market_token(market_token)
                    .params(&CreateDepositParams {
                        execution_lamports: params.execution_lamports,
                        long_token_swap_length: params.long_token_swap_length,
                        short_token_swap_length: params.short_token_swap_length,
                        initial_long_token_amount: params.initial_long_token_amount,
                        initial_short_token_amount: params.initial_short_token_amount,
                        min_market_token_amount: params.min_market_token_amount,
                        should_unwrap_native_token: params.should_unwrap_native_token,
                    })
                    .long_pay_token(long_pay_token)
                    .long_swap_path(long_swap_path)
                    .short_pay_token(short_pay_token)
                    .short_swap_path(short_swap_path)
                    .build()
                    .execute_with_options(options)
            })
            .transpose()?;

        let market_token_amount = params
            .market_token_amount
            .checked_add(
                deposit_output
                    .as_ref()
                    .map(|d| *d.report().minted())
                    .unwrap_or(0)
                    .try_into()
                    .map_err(|_| {
                        crate::Error::custom("[sim] normal deposit output amount overflow")
                    })?,
            )
            .ok_or(crate::Error::custom("[sim] market token amount overflow"))?;

        if market_token_amount == 0 {
            if is_deposit_empty {
                return Err(crate::Error::custom(
                    "[sim] insufficient deposit output amount",
                ));
            } else {
                return Err(crate::Error::custom("[sim] empty GLV deposit"));
            }
        }

        let glv_value = simulator.get_glv_value(glv_token, true)?;
        let (market, prices) = simulator.get_market_with_prices(market_token)?;
        let received_value = gmsol_model::glv::get_glv_value_for_market(
            &prices,
            market,
            market_token_amount.into(),
            false,
        )?
        .market_token_value_in_glv;

        let minted = simulator
            .get_glv_mut(glv_token)
            .expect("must exist")
            .deposit(market_token, market_token_amount, received_value, glv_value)?;

        let min_glv_token_amount = params.min_glv_token_amount;
        if minted < min_glv_token_amount {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient output amount: {minted} < {min_glv_token_amount}",
            )));
        }

        match deposit_output {
            Some(output) => {
                let DepositSimulationOutput {
                    long_swaps,
                    short_swaps,
                    report,
                } = output;
                Ok(GlvDepositSimulationOutput {
                    long_swaps,
                    short_swaps,
                    deposit_report: Some(report),
                    output_amount: minted,
                })
            }
            None => Ok(GlvDepositSimulationOutput {
                long_swaps: Default::default(),
                short_swaps: Default::default(),
                deposit_report: None,
                output_amount: minted,
            }),
        }
    }
}
