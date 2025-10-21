use gmsol_model::LiquidityMarketMutExt;
use gmsol_model::MarketAction;
use solana_sdk::pubkey::Pubkey;

use crate::{simulation::Simulator, utils::base64::encode_base64};

#[derive(Debug, Clone)]
pub struct GmDepositExactOutput {
    pub minted: u128,
    pub report_borsh_base64: String,
}

#[derive(Debug, Clone)]
pub struct GmWithdrawalExactOutput {
    pub long_out: u128,
    pub short_out: u128,
    pub report_borsh_base64: String,
}

pub fn simulate_gm_deposit_exact(
    sim: &mut Simulator,
    market_token: &Pubkey,
    long_in: u128,
    short_in: u128,
    include_virtual_inventory_impact: bool,
) -> crate::Result<GmDepositExactOutput> {
    let (market, prices) = sim.get_market_with_prices_mut(market_token)?;
    let report = market
        .deposit(long_in, short_in, prices)?
        .with_virtual_inventory_impact(include_virtual_inventory_impact)
        .execute()?;
    let minted = *report.minted();
    let report_borsh_base64 = borsh_to_base64(&report)?;
    Ok(GmDepositExactOutput {
        minted,
        report_borsh_base64,
    })
}

pub fn simulate_gm_withdrawal_exact(
    sim: &mut Simulator,
    market_token: &Pubkey,
    market_token_amount: u128,
) -> crate::Result<GmWithdrawalExactOutput> {
    let (market, prices) = sim.get_market_with_prices_mut(market_token)?;
    let report = market.withdraw(market_token_amount, prices)?.execute()?;
    let long_out = *report.long_token_output();
    let short_out = *report.short_token_output();
    let report_borsh_base64 = borsh_to_base64(&report)?;
    Ok(GmWithdrawalExactOutput {
        long_out,
        short_out,
        report_borsh_base64,
    })
}

fn borsh_to_base64<T: borsh::BorshSerialize>(data: &T) -> crate::Result<String> {
    data.try_to_vec()
        .map(|bytes| encode_base64(&bytes))
        .map_err(crate::Error::custom)
}

use borsh::BorshSerialize;
use gmsol_model::utils::{market_token_amount_to_usd, usd_to_market_token_amount};
use gmsol_programs::constants::MARKET_USD_TO_AMOUNT_DIVISOR;

#[derive(Debug, Clone)]
pub struct GlvComponentSim {
    pub market_token: Pubkey,
    pub balance: u64,
    pub pool_value: u128,
    pub supply: u64,
}

#[derive(Debug, Clone, BorshSerialize)]
struct GlvPricingReport {
    pub supply: u64,
    pub is_value_maximized: bool,
    pub total_value: u128,
    pub market_token: Pubkey,
    pub input_amount: u64,
    pub input_value: u128,
    pub output_amount: u64,
}

#[derive(Debug, Clone)]
pub struct GlvDepositExactOutputSim {
    pub glv_minted: u64,
    pub market_token_delta: u64,
    pub pricing_report_borsh_base64: String,
}

#[derive(Debug, Clone)]
pub struct GlvWithdrawalExactOutputSim {
    pub market_token_amount: u64,
    pub long_out: u128,
    pub short_out: u128,
    pub pricing_report_borsh_base64: String,
}

fn sum_glv_total_value(components: &[GlvComponentSim]) -> crate::Result<u128> {
    let mut total: u128 = 0;
    for c in components {
        let v = market_token_amount_to_usd(
            &u128::from(c.balance),
            &c.pool_value,
            &u128::from(c.supply),
        )
        .ok_or_else(|| crate::Error::custom("failed to compute component value"))?;
        total = total
            .checked_add(v)
            .ok_or_else(|| crate::Error::custom("overflow computing GLV total value"))?;
    }
    Ok(total)
}

fn find_component<'a>(
    components: &'a [GlvComponentSim],
    market_token: &Pubkey,
) -> crate::Result<&'a GlvComponentSim> {
    components
        .iter()
        .find(|c| &c.market_token == market_token)
        .ok_or_else(|| crate::Error::custom("current market not found in GLV components"))
}

pub fn simulate_glv_deposit_exact(
    sim: &mut Simulator,
    market_token: &Pubkey,
    long_in: u128,
    short_in: u128,
    additional_market_token_amount: u128,
    include_virtual_inventory_impact: bool,
    glv_supply: u64,
    components: &[GlvComponentSim],
) -> crate::Result<GlvDepositExactOutputSim> {
    let gm = simulate_gm_deposit_exact(
        sim,
        market_token,
        long_in,
        short_in,
        include_virtual_inventory_impact,
    )?;
    let minted_u64: u64 = gm
        .minted
        .try_into()
        .map_err(|_| crate::Error::custom("minted overflow u64"))?;
    let add_mt_u64: u64 = additional_market_token_amount
        .try_into()
        .map_err(|_| crate::Error::custom("market_token_amount overflow u64"))?;
    let market_token_delta = minted_u64
        .checked_add(add_mt_u64)
        .ok_or_else(|| crate::Error::custom("overflow add market_token_delta"))?;

    let current = find_component(components, market_token)?;
    let total_value = sum_glv_total_value(components)?;
    let received_value = market_token_amount_to_usd(
        &u128::from(market_token_delta),
        &current.pool_value,
        &u128::from(current.supply),
    )
    .ok_or_else(|| crate::Error::custom("failed to compute received_value"))?;

    let glv_value = total_value;
    let glv_amount_u128 = usd_to_market_token_amount(
        received_value,
        glv_value,
        u128::from(glv_supply),
        MARKET_USD_TO_AMOUNT_DIVISOR,
    )
    .ok_or_else(|| crate::Error::custom("failed to calculate glv amount to mint"))?;
    let glv_minted: u64 = glv_amount_u128
        .try_into()
        .map_err(|_| crate::Error::custom("glv minted overflow u64"))?;

    let pricing_report = GlvPricingReport {
        supply: glv_supply,
        is_value_maximized: true,
        total_value: glv_value,
        market_token: *market_token,
        input_amount: market_token_delta,
        input_value: received_value,
        output_amount: glv_minted,
    };
    let pricing_report_borsh_base64 = borsh_to_base64(&pricing_report)?;

    Ok(GlvDepositExactOutputSim {
        glv_minted,
        market_token_delta,
        pricing_report_borsh_base64,
    })
}

pub fn simulate_glv_withdrawal_exact(
    sim: &mut Simulator,
    market_token: &Pubkey,
    glv_token_amount: u64,
    glv_supply: u64,
    components: &[GlvComponentSim],
) -> crate::Result<GlvWithdrawalExactOutputSim> {
    let current = find_component(components, market_token)?;
    let total_value = sum_glv_total_value(components)?;

    let market_token_value = market_token_amount_to_usd(
        &u128::from(glv_token_amount),
        &total_value,
        &u128::from(glv_supply),
    )
    .ok_or_else(|| {
        crate::Error::custom("failed to calculate market_token_value for glv withdrawal")
    })?;

    let market_token_amount_u128 = usd_to_market_token_amount(
        market_token_value,
        current.pool_value,
        u128::from(current.supply),
        MARKET_USD_TO_AMOUNT_DIVISOR,
    )
    .ok_or_else(|| {
        crate::Error::custom("failed to calculate market_token_amount for glv withdrawal")
    })?;
    let market_token_amount: u64 = market_token_amount_u128
        .try_into()
        .map_err(|_| crate::Error::custom("market_token_amount overflow u64"))?;

    let gm = simulate_gm_withdrawal_exact(sim, market_token, u128::from(market_token_amount))?;

    let pricing_report = GlvPricingReport {
        supply: glv_supply,
        is_value_maximized: false,
        total_value,
        market_token: *market_token,
        input_amount: glv_token_amount,
        input_value: market_token_value,
        output_amount: market_token_amount,
    };
    let pricing_report_borsh_base64 = borsh_to_base64(&pricing_report)?;

    Ok(GlvWithdrawalExactOutputSim {
        market_token_amount,
        long_out: gm.long_out,
        short_out: gm.short_out,
        pricing_report_borsh_base64,
    })
}
