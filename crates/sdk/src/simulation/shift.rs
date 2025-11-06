use gmsol_model::{
    action::{deposit::DepositReport, withdraw::WithdrawReport},
    LiquidityMarketMutExt, MarketAction,
};
use gmsol_programs::{gmsol_store::types::CreateShiftParams, model::SwapPricingKind};
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use super::{SimulationOptions, Simulator};

/// Shift simulation output.
#[derive(Debug)]
pub struct ShiftSimulationOutput {
    pub(crate) withdraw: Box<WithdrawReport<u128>>,
    pub(crate) deposit: Box<DepositReport<u128, i128>>,
}

impl ShiftSimulationOutput {
    /// Returns the withdrawal report.
    pub fn withdraw(&self) -> &WithdrawReport<u128> {
        &self.withdraw
    }

    /// Returns the deposit report.
    pub fn deposit(&self) -> &DepositReport<u128, i128> {
        &self.deposit
    }
}

/// Shift execution simulation.
#[derive(Debug, TypedBuilder)]
pub struct ShiftSimulation<'a> {
    simulator: &'a mut Simulator,
    params: &'a CreateShiftParams,
    from_market_token: &'a Pubkey,
    to_market_token: &'a Pubkey,
}

impl ShiftSimulation<'_> {
    /// Execute with options.
    pub fn execute_with_options(
        self,
        _options: SimulationOptions,
    ) -> crate::Result<ShiftSimulationOutput> {
        let Self {
            simulator,
            params,
            from_market_token,
            to_market_token,
        } = self;

        if params.from_market_token_amount == 0 {
            return Err(crate::Error::custom("[sim] empty shift"));
        }

        let (from_market, prices_for_from_market) =
            simulator.get_market_with_prices(from_market_token)?;
        let (to_market, prices_for_to_market) =
            simulator.get_market_with_prices(to_market_token)?;

        if from_market.meta.long_token_mint != to_market.meta.long_token_mint
            || from_market.meta.short_token_mint != to_market.meta.short_token_mint
        {
            return Err(crate::Error::custom(format!(
                "[sim] shift from `{from_market_token}` to `{to_market_token}` is impossible"
            )));
        }

        // Execute withdrawal.
        let from_market = simulator
            .get_market_mut(from_market_token)
            .expect("must exist");

        let withdraw = from_market.with_swap_pricing(SwapPricingKind::Shift, |market| {
            market
                .withdraw(
                    params.from_market_token_amount.into(),
                    prices_for_from_market,
                )?
                .execute()
        })?;

        let (long_token_amount, short_token_amount) = (
            *withdraw.long_token_output(),
            *withdraw.short_token_output(),
        );

        if long_token_amount == 0 && short_token_amount == 0 {
            return Err(crate::Error::custom(
                "[sim] shift cannot be completed due to empty withdrawal output",
            ));
        }

        // Execute deposit.
        let to_market = simulator
            .get_market_mut(to_market_token)
            .expect("must exist");
        let deposit = to_market.with_swap_pricing(SwapPricingKind::Shift, |market| {
            market
                .deposit(long_token_amount, short_token_amount, prices_for_to_market)?
                .execute()
        })?;

        let minted = deposit.minted();
        let min_to_market_token_amount = params.min_to_market_token_amount;
        if *minted < u128::from(min_to_market_token_amount) {
            return Err(crate::Error::custom(format!(
                "[sim] insufficient output amount: {minted} < {min_to_market_token_amount}",
            )));
        }

        Ok(ShiftSimulationOutput {
            withdraw: Box::new(withdraw),
            deposit: Box::new(deposit),
        })
    }
}
