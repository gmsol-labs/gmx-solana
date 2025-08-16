const SECONDS_PER_YEAR: u128 = 31_557_600; // 365.25 * 24 * 3600
const PERCENT_DIVISOR: u128 = 100;
const APY_PER_SEC_DENOM: u128 = PERCENT_DIVISOR * SECONDS_PER_YEAR;
use anchor_lang::prelude::*;
use gmsol_model::utils::apply_factor;
use gmsol_programs::gmsol_store::constants::MARKET_USD_UNIT;
use gmsol_programs::gmsol_store::{
    accounts::Store, cpi as gt_cpi, cpi::accounts::UpdateGtCumulativeInvCostFactor as GtUpdateCtx,
    cpi::Return as GtReturn, program::GmsolStore,
};

declare_id!("BGDJg2u2NWwUE5q4Q4masGCFBVAhJ5pKrMbVSwjVwo8m");

#[program]
pub mod gmsol_liquidity_provider {
    use super::*;

    /// Initialize LP staking program
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        global_state.authority = ctx.accounts.authority.key();
        global_state.gt_mint = ctx.accounts.gt_mint.key();
        // APY as whole percent, price as 1e20
        global_state.gt_apy = 15; // 15% APY (unscaled percentage)
        global_state.gt_price_usd = MARKET_USD_UNIT; // $1.00 in 1e20 units
        msg!("LP staking program initialized, GT APY: 15%, GT price: $1.00");
        Ok(())
    }

    /// Calculate GT rewards for LP
    pub fn calculate_gt_reward(
        ctx: Context<CalculateGtReward>,
        lp_staked_amount: u64,
        stake_start_time: i64,
        start_cum_inv_cost: u128,
    ) -> Result<()> {
        let global_state = &ctx.accounts.global_state;
        let current_time = Clock::get()?.unix_timestamp;

        // duration in seconds (non-negative)
        let duration_seconds = current_time.saturating_sub(stake_start_time);

        // --- Update GT cumulative inverse cost factor via CPI and read current cumulative value ---
        // Use the GlobalState PDA as GT_CONTROLLER authority
        let gs_bump: u8 = ctx.bumps.global_state;
        let gs_seeds: &[&[u8]] = &[b"global_state", &[gs_bump]];
        let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.gt_program.to_account_info(),
            GtUpdateCtx {
                authority: ctx.accounts.global_state.to_account_info(),
                store: ctx.accounts.gt_store.to_account_info(),
            },
            signer_seeds,
        );

        let r: GtReturn<u128> = gt_cpi::update_gt_cumulative_inv_cost_factor(cpi_ctx)?;
        let cum_now: u128 = r.get();

        // Integral over [start, now] (C(now) - C(start)); require monotonicity to avoid masking bugs
        require!(cum_now >= start_cum_inv_cost, ErrorCode::InvalidArgument);
        let inv_cost_integral = cum_now - start_cum_inv_cost;

        msg!(
            "GT inverse-cost cumulative: start={}, now={}, integral={}",
            start_cum_inv_cost,
            cum_now,
            inv_cost_integral
        );

        // In this version, treat lp_staked_amount as raw amount, convert to scaled USD value
        let staked_value_usd = (lp_staked_amount as u128).saturating_mul(MARKET_USD_UNIT);

        // Read GT decimals from GT store and derive GT amount unit (10^decimals)
        let gt_decimals_u8 = {
            let store = ctx.accounts.gt_store.load()?;
            store.gt.decimals
        };
        let gt_decimals: u32 = gt_decimals_u8 as u32;
        let gt_amount_unit: u128 = 10u128.pow(gt_decimals);

        // Calculate GT reward amount in raw base units (decimals = 7)
        let gt_reward_raw = calculate_gt_reward_amount(
            staked_value_usd,
            duration_seconds,
            global_state.gt_apy as u128,
            inv_cost_integral,
            gt_amount_unit,
        )?;

        // For display, also compute human-readable GT (floored)
        let gt_whole = (gt_reward_raw / (gt_amount_unit as u64)) as u64;

        msg!("Staked amount (scaled USD): {}", staked_value_usd);
        msg!("Staking duration (s): {}", duration_seconds);
        msg!("GT APY (whole %): {}", global_state.gt_apy);
        msg!(
            "Calculated GT reward (raw, decimals={}): {}",
            gt_decimals,
            gt_reward_raw
        );
        msg!("Calculated GT reward (whole GT, floored): {} GT", gt_whole);
        Ok(())
    }

    /// Update GT APY parameter
    pub fn update_gt_apy(
        ctx: Context<UpdateGtApy>,
        new_apy: u64, // APY as whole percent, e.g., 15 for 15%
    ) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(
            ctx.accounts.authority.key() == global_state.authority,
            ErrorCode::Unauthorized
        );
        global_state.gt_apy = new_apy;
        msg!("GT APY (whole %) updated to: {}", new_apy);
        Ok(())
    }
}

/// Calculate GT reward amount (returns raw amount in base units, respecting token decimals)
/// Expects the integral over the window: inv_cost_integral = C(now) - C(start), where
/// C(t) accumulates (MARKET_USD_UNIT / price(t)) dt. No extra multiply by `duration_seconds`
/// is required because time is already integrated inside `inv_cost_integral`.
fn calculate_gt_reward_amount(
    staked_value_usd: u128,  // Already scaled USD value (e.g., in MARKET_USD_UNIT)
    duration_seconds: i64,   // current_time - stake_start_time
    gt_apy: u128,            // APY as whole percent (e.g., 15 for 15%)
    inv_cost_integral: u128, // ∫ (MARKET_USD_UNIT / price(t)) dt over the interval [start, now]
    gt_amount_unit: u128,
) -> Result<u64> {
    require!(duration_seconds >= 0, ErrorCode::Unauthorized);

    let dur = duration_seconds as u128;

    let per_sec_factor = staked_value_usd
        .saturating_mul(gt_apy)
        .saturating_mul(gt_amount_unit) // to GT base units later
        .saturating_div(APY_PER_SEC_DENOM);

    // Apply the integral of MARKET_USD_UNIT / price(t) over time.
    // Units:
    // inv_cost_integral has units of seconds * (MARKET_USD_UNIT / USD_scaled).
    // Multiplying by 'per_sec_factor' (USD_scaled/sec in GT base units after previous step) cancels seconds and MARKET_USD_UNIT.
    // apply_factor::<T, DECIMALS>(&value, &factor) where DECIMALS is the decimals of `factor`.
    // Here, `factor` = inv_cost_integral has MARKET_USD_UNIT (1e20) decimals.
    let gt_raw = apply_factor::<u128, 20>(&per_sec_factor, &inv_cost_integral)
        .ok_or(ErrorCode::MathOverflow)?;

    Ok(gt_raw.min(u64::MAX as u128) as u64)
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + GlobalState::INIT_SPACE,
        seeds = [b"global_state"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,

    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: GT token mint address
    pub gt_mint: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CalculateGtReward<'info> {
    #[account(seeds = [b"global_state"], bump)]
    pub global_state: Account<'info, GlobalState>,
    /// The GT Store account (loaded & mutated by CPI)
    #[account(mut)]
    pub gt_store: AccountLoader<'info, Store>,
    /// The GT program
    pub gt_program: Program<'info, GmsolStore>,
}

#[derive(Accounts)]
pub struct UpdateGtApy<'info> {
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateGtPrice<'info> {
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,

    pub authority: Signer<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct GlobalState {
    /// Program administrator
    pub authority: Pubkey,
    /// GT token mint address
    pub gt_mint: Pubkey,
    /// APY as whole percent (e.g., 15 for 15%)
    pub gt_apy: u64,
    /// USD price scaled by 1e20
    pub gt_price_usd: u128,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Unauthorized operation")]
    Unauthorized,
    #[msg("Invalid argument")]
    InvalidArgument,
    #[msg("Math overflow")]
    MathOverflow,
}
