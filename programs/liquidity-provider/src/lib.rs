use anchor_lang::prelude::AccountsClose;
use anchor_lang::prelude::*;
use anchor_spl::token_interface as token_if;
use anchor_spl::token_interface::{
    CloseAccount, Mint, TokenAccount, TokenInterface, TransferChecked,
};
use gmsol_model::num::MulDiv;
use gmsol_model::utils::apply_factor;
use gmsol_programs::gmsol_store::constants::{MARKET_DECIMALS, MARKET_USD_UNIT};

#[constant]
pub const POSITION_SEED: &'static [u8] = b"position";
#[constant]
pub const GLOBAL_STATE_SEED: &'static [u8] = b"global_state";
#[constant]
pub const VAULT_SEED: &'static [u8] = b"vault";

use gmsol_programs::gmsol_store::{
    accounts::{Store, UserHeader},
    cpi as gt_cpi,
    cpi::accounts::{MintGtReward as GtMintCtx, UpdateGtCumulativeInvCostFactor as GtUpdateCtx},
    cpi::Return as GtReturn,
    program::GmsolStore,
};

const SECONDS_PER_YEAR: u128 = 31_557_600; // 365.25 * 24 * 3600
const SECONDS_PER_WEEK: u128 = 7 * 24 * 3600;

declare_id!("BGDJg2u2NWwUE5q4Q4masGCFBVAhJ5pKrMbVSwjVwo8m");

#[program]
pub mod gmsol_liquidity_provider {
    use super::*;

    /// Initialize LP staking program
    pub fn initialize(
        ctx: Context<Initialize>,
        min_stake_value: u128,
        apy_gradient: [u128; 53], // each item is 1e20-scaled APR for week buckets [0-1), [1-2), ..., [52, +inf)
    ) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        global_state.authority = ctx.accounts.authority.key();
        global_state.pending_authority = Pubkey::default();
        global_state.gt_mint = ctx.accounts.gt_mint.key();
        global_state.apy_gradient = apy_gradient;
        global_state.lp_token_price = MARKET_USD_UNIT; // $1.00 in 1e20 units
        global_state.min_stake_value = min_stake_value;
        global_state.bump = ctx.bumps.global_state;
        msg!(
            "LP staking program initialized, min_stake_value(1e20)={}",
            min_stake_value
        );
        Ok(())
    }

    /// Create a new LP position and snapshot stake-time values
    pub fn stake_lp(
        ctx: Context<StakeLp>,
        position_id: u64,
        lp_staked_amount: u64,
        lp_staked_value: u128, // scaled USD at stake time
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;

        // Enforce minimum stake value (scaled 1e20)
        require!(
            lp_staked_value >= ctx.accounts.global_state.min_stake_value,
            ErrorCode::InvalidArgument
        );

        // Use GlobalState PDA as controller for GT CPI
        let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[ctx.accounts.global_state.bump]];
        let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.gt_program.to_account_info(),
            GtUpdateCtx {
                authority: ctx.accounts.global_state.to_account_info(),
                store: ctx.accounts.gt_store.to_account_info(),
            },
            signer_seeds,
        );
        // Snapshot C(start) at stake time
        let r: GtReturn<u128> = gt_cpi::update_gt_cumulative_inv_cost_factor(cpi_ctx)?;
        let c_start: u128 = r.get();

        // Transfer LP tokens from user to the position vault
        if lp_staked_amount > 0 {
            let cpi_accounts = TransferChecked {
                from: ctx.accounts.user_lp_token.to_account_info(),
                mint: ctx.accounts.lp_mint.to_account_info(),
                to: ctx.accounts.position_vault.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            };
            let cpi_ctx =
                CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
            token_if::transfer_checked(cpi_ctx, lp_staked_amount, ctx.accounts.lp_mint.decimals)?;
        }

        // Init position fields
        let position = &mut ctx.accounts.position;
        position.owner = ctx.accounts.owner.key();
        position.global_state = ctx.accounts.global_state.key();
        position.lp_mint = ctx.accounts.lp_mint.key();
        position.vault = ctx.accounts.position_vault.key();
        position.position_id = position_id;
        position.staked_amount = lp_staked_amount;
        position.staked_value_usd = lp_staked_value;
        position.stake_start_time = now;
        position.cum_inv_cost = c_start;
        position.bump = ctx.bumps.position;

        msg!(
            "Stake created: owner={}, amount={}, value(1e20)={}, start_ts={}, C_start={}, pos_id={}",
            position.owner,
            lp_staked_amount,
            lp_staked_value,
            now,
            c_start,
            position_id
        );
        Ok(())
    }

    /// Calculate GT rewards for LP based on stored Position data (no mint)
    pub fn calculate_gt_reward(ctx: Context<CalculateGtReward>) -> Result<()> {
        // Refresh C(t) via CPI and compute reward using shared helper
        let out = compute_reward_with_cpi(
            &ctx.accounts.global_state,
            &ctx.accounts.gt_store,
            &ctx.accounts.gt_program,
            &ctx.accounts.position,
        )?;
        let cum_now = out.cum_now;
        let inv_cost_integral = out.inv_cost_integral;
        let gt_reward_raw = out.gt_reward_raw;

        // For display, also compute human-readable GT (floored)
        let gt_decimals_u8 = {
            let store = ctx.accounts.gt_store.load()?;
            store.gt.decimals
        };
        let gt_decimals: u32 = gt_decimals_u8 as u32;
        let gt_amount_unit: u128 = 10u128.pow(gt_decimals);
        let gt_whole = (gt_reward_raw / (gt_amount_unit as u64)) as u64;

        msg!(
            "GT inverse-cost cumulative: start={}, now={}, integral={}",
            ctx.accounts.position.cum_inv_cost,
            cum_now,
            inv_cost_integral
        );
        msg!("Calculated GT reward (whole GT, floored): {} GT", gt_whole);
        Ok(())
    }

    /// Claim GT rewards for a position, minting tokens and updating snapshot
    pub fn claim_gt(ctx: Context<ClaimGt>, _position_id: u64) -> Result<()> {
        let global_state = &ctx.accounts.global_state;

        // Refresh C(t) via CPI and compute reward using shared helper
        let out = compute_reward_with_cpi(
            &ctx.accounts.global_state,
            &ctx.accounts.store,
            &ctx.accounts.gt_program,
            &ctx.accounts.position,
        )?;
        let gt_reward_raw = out.gt_reward_raw;
        let cum_now = out.cum_now;
        let prev_cum = out.prev_cum;
        let inv_cost_integral = out.inv_cost_integral;
        let duration_seconds = out.duration_seconds;
        // Capture position id immutably for later log use
        let pos_id = ctx.accounts.position.position_id;

        // Mint GT tokens to the user's GT account (authority = GlobalState PDA)
        if gt_reward_raw > 0 {
            let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
            let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

            let mint_ctx = CpiContext::new_with_signer(
                ctx.accounts.gt_program.to_account_info(),
                GtMintCtx {
                    authority: global_state.to_account_info(),
                    store: ctx.accounts.store.to_account_info(),
                    user: ctx.accounts.gt_user.to_account_info(),
                    event_authority: ctx.accounts.event_authority.to_account_info(),
                    program: ctx.accounts.gt_program.to_account_info(),
                },
                signer_seeds,
            );
            gt_cpi::mint_gt_reward(mint_ctx, gt_reward_raw)?;
        }

        // Update snapshot to now for future claims (do NOT change stake_start_time)
        {
            let position = &mut ctx.accounts.position;
            position.cum_inv_cost = cum_now;
        }

        msg!(
            "Claimed GT: amount_raw={} | pos_id={} | duration={}s | C_prev->C_now: {}->{}, integral={}",
            gt_reward_raw,
            pos_id,
            duration_seconds,
            prev_cum,
            cum_now,
            inv_cost_integral
        );

        Ok(())
    }

    /// Unstake LP: first claim rewards, then either close the position (full) or update proportionally (partial)
    pub fn unstake_lp(
        ctx: Context<UnstakeLp>,
        _position_id: u64,
        unstake_amount: u64,
    ) -> Result<()> {
        require!(unstake_amount > 0, ErrorCode::InvalidArgument);

        let global_state = &ctx.accounts.global_state;

        // 1) Claim-like flow: refresh C(t), compute reward, mint, and snapshot
        let out = compute_reward_with_cpi(
            &ctx.accounts.global_state,
            &ctx.accounts.store,
            &ctx.accounts.gt_program,
            &ctx.accounts.position,
        )?;
        let gt_reward_raw = out.gt_reward_raw;
        let cum_now = out.cum_now;
        let prev_cum = out.prev_cum;
        let inv_cost_integral = out.inv_cost_integral;

        if gt_reward_raw > 0 {
            let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
            let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

            let mint_ctx = CpiContext::new_with_signer(
                ctx.accounts.gt_program.to_account_info(),
                GtMintCtx {
                    authority: global_state.to_account_info(),
                    store: ctx.accounts.store.to_account_info(),
                    user: ctx.accounts.gt_user.to_account_info(),
                    event_authority: ctx.accounts.event_authority.to_account_info(),
                    program: ctx.accounts.gt_program.to_account_info(),
                },
                signer_seeds,
            );
            gt_cpi::mint_gt_reward(mint_ctx, gt_reward_raw)?;
        }

        // Snapshot to now
        {
            let position = &mut ctx.accounts.position;
            position.cum_inv_cost = cum_now;
        }

        // 2) Apply unstake amount
        let (old_amount, old_value) = {
            let p = &ctx.accounts.position;
            (p.staked_amount, p.staked_value_usd)
        };
        require!(unstake_amount <= old_amount, ErrorCode::InvalidArgument);

        // Sanity: ensure the passed lp_mint matches the position
        require_keys_eq!(
            ctx.accounts.lp_mint.key(),
            ctx.accounts.position.lp_mint,
            ErrorCode::InvalidArgument
        );

        let remaining_amount = old_amount.saturating_sub(unstake_amount);

        // Compute new_value for partial case and determine if we should fully exit
        let new_value = if remaining_amount == 0 {
            0
        } else {
            MulDiv::checked_mul_div(
                &old_value,
                &(remaining_amount as u128),
                &(old_amount as u128),
            )
            .ok_or(ErrorCode::MathOverflow)?
        };
        let full_exit =
            remaining_amount == 0 || new_value < ctx.accounts.global_state.min_stake_value;

        // Decide transfer amount based on full_exit
        let amount_to_transfer = if full_exit {
            old_amount
        } else {
            unstake_amount
        };
        if amount_to_transfer > 0 {
            let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
            let signer_seeds: &[&[&[u8]]] = &[gs_seeds];
            let cpi_accounts = TransferChecked {
                from: ctx.accounts.position_vault.to_account_info(),
                mint: ctx.accounts.lp_mint.to_account_info(),
                to: ctx.accounts.user_lp_token.to_account_info(),
                authority: ctx.accounts.global_state.to_account_info(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );
            token_if::transfer_checked(cpi_ctx, amount_to_transfer, ctx.accounts.lp_mint.decimals)?;
        }

        if full_exit {
            // Full unstake: zero fields, close vault, close position account
            {
                let position = &mut ctx.accounts.position;
                position.staked_amount = 0;
                position.staked_value_usd = 0;
            }
            // Close the vault token account to return rent to owner
            let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
            let signer_seeds: &[&[&[u8]]] = &[gs_seeds];
            let close_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                CloseAccount {
                    account: ctx.accounts.position_vault.to_account_info(),
                    destination: ctx.accounts.owner.to_account_info(),
                    authority: ctx.accounts.global_state.to_account_info(),
                },
                signer_seeds,
            );
            token_if::close_account(close_ctx)?;

            ctx.accounts
                .position
                .close(ctx.accounts.owner.to_account_info())?;
        } else {
            // Partial update
            let position = &mut ctx.accounts.position;
            position.staked_amount = remaining_amount;
            position.staked_value_usd = new_value;

            msg!(
                "Unstaked partial: old_amount={}, unstake={}, remain={}, value_scaled={} (C_prev->C_now: {}->{}, integral={}, reward_raw={})",
                old_amount,
                unstake_amount,
                remaining_amount,
                new_value,
                prev_cum,
                cum_now,
                inv_cost_integral,
                gt_reward_raw
            );
        }

        Ok(())
    }

    /// Update the minimum stake value (1e20 scaled)
    pub fn update_min_stake_value(
        ctx: Context<UpdateMinStakeValue>,
        new_min_stake_value: u128,
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        gs.min_stake_value = new_min_stake_value;
        msg!("min_stake_value updated to (1e20): {}", new_min_stake_value);
        Ok(())
    }

    /// Update APY gradient array (53 buckets)
    pub fn update_apy_gradient(
        ctx: Context<UpdateApyGradient>,
        new_apy_gradient: [u128; 53],
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        gs.apy_gradient = new_apy_gradient;
        msg!("apy_gradient updated (53 buckets)");
        Ok(())
    }

    /// Propose transferring program authority to `new_authority` (two-step handover).
    pub fn transfer_authority(
        ctx: Context<TransferAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        require!(
            new_authority != Pubkey::default(),
            ErrorCode::InvalidArgument
        );
        gs.pending_authority = new_authority;
        msg!(
            "Authority transfer proposed: pending_authority = {}",
            new_authority
        );
        Ok(())
    }

    /// Accept authority if you are the pending_authority; finalizes the handover.
    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        gs.authority = ctx.accounts.pending_authority.key();
        gs.pending_authority = Pubkey::default();
        msg!(
            "Authority transfer accepted: new authority = {}",
            gs.authority
        );
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
    gt_apy_per_sec: u128,    // per-second factor scaled by 1e20
    inv_cost_integral: u128, // ∫ (MARKET_USD_UNIT / price(t)) dt over the interval [start, now]
) -> Result<u64> {
    require!(duration_seconds >= 0, ErrorCode::Unauthorized);

    // Convert notional USD to per-second notionals using APY per second
    let per_sec_factor = apply_factor::<u128, MARKET_DECIMALS>(&staked_value_usd, &gt_apy_per_sec)
        .ok_or(ErrorCode::MathOverflow)?;

    // Apply the integral of MARKET_USD_UNIT / price(t) over time.
    let gt_raw = apply_factor::<u128, MARKET_DECIMALS>(&per_sec_factor, &inv_cost_integral)
        .ok_or(ErrorCode::MathOverflow)?;

    Ok(gt_raw.min(u64::MAX as u128) as u64)
}

/// Output of reward computation with CPI-updated cumulative inverse cost
struct ComputeRewardOut {
    gt_reward_raw: u64,
    cum_now: u128,
    prev_cum: u128,
    inv_cost_integral: u128,
    duration_seconds: i64,
}

/// Compute reward by (a) refreshing C(t) via GT CPI and (b) applying APY-per-sec and integral.
fn compute_reward_with_cpi<'info>(
    global_state: &Account<'info, GlobalState>,
    store: &AccountLoader<'info, Store>,
    gt_program: &Program<'info, GmsolStore>,
    position: &Account<'info, Position>,
) -> Result<ComputeRewardOut> {
    // Use GlobalState PDA as controller for GT CPI
    let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
    let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

    let update_ctx = CpiContext::new_with_signer(
        gt_program.to_account_info(),
        GtUpdateCtx {
            authority: global_state.to_account_info(),
            store: store.to_account_info(),
        },
        signer_seeds,
    );

    // 1) Refresh cumulative inverse cost and read C(now)
    let r: GtReturn<u128> = gt_cpi::update_gt_cumulative_inv_cost_factor(update_ctx)?;
    let cum_now: u128 = r.get();
    let prev_cum: u128 = position.cum_inv_cost;

    // 2) Compute integral over [last_snapshot, now]
    require!(cum_now >= prev_cum, ErrorCode::InvalidArgument);
    let inv_cost_integral = cum_now - prev_cum;

    // 3) Duration and time-weighted APY
    let current_time = Clock::get()?.unix_timestamp;
    let duration_seconds = current_time.saturating_sub(position.stake_start_time);
    let avg_apy = compute_time_weighted_apy(
        position.stake_start_time,
        current_time,
        &global_state.apy_gradient,
    );
    let avg_apy_per_sec = if SECONDS_PER_YEAR > 0 {
        avg_apy / SECONDS_PER_YEAR
    } else {
        0
    };

    // 4) Reward in GT base units
    let gt_reward_raw = calculate_gt_reward_amount(
        position.staked_value_usd,
        duration_seconds,
        avg_apy_per_sec,
        inv_cost_integral,
    )?;

    Ok(ComputeRewardOut {
        gt_reward_raw,
        cum_now,
        prev_cum,
        inv_cost_integral,
        duration_seconds,
    })
}

/// Compute time-weighted average APR over [start, now] using 53-bucket weekly gradient (1e20-scaled).
fn compute_time_weighted_apy(stake_start_time: i64, now: i64, apy_gradient: &[u128; 53]) -> u128 {
    if now <= stake_start_time {
        return apy_gradient[0];
    }
    let mut remaining: u128 = (now - stake_start_time) as u128;
    let mut current_start: i64 = stake_start_time;
    let mut acc: u128 = 0;
    let mut week_index: usize = 0;

    while remaining > 0 {
        let week_end = current_start.saturating_add(SECONDS_PER_WEEK as i64);
        let segment_seconds: u128 = if (now as i128) < (week_end as i128) {
            remaining
        } else {
            SECONDS_PER_WEEK.min(remaining)
        };
        let apy = if week_index < 52 {
            apy_gradient[week_index]
        } else {
            apy_gradient[52]
        };
        // accumulate apy * seconds
        acc = acc.saturating_add(apy.saturating_mul(segment_seconds));
        remaining = remaining.saturating_sub(segment_seconds);
        current_start = week_end;
        week_index = week_index.saturating_add(1);
    }

    let total_seconds: u128 = (now - stake_start_time) as u128;
    if total_seconds == 0 {
        apy_gradient[0]
    } else {
        acc / total_seconds
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + GlobalState::INIT_SPACE,
        seeds = [GLOBAL_STATE_SEED],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,

    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: GT token mint address
    pub gt_mint: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Accounts context for staking LP tokens and creating a Position
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct StakeLp<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Account<'info, GlobalState>,

    /// LP token mint to be staked
    pub lp_mint: InterfaceAccount<'info, Mint>,

    /// Position PDA to initialize for (global_state, owner, position_id)
    #[account(
        init,
        payer = owner,
        space = 8 + Position::INIT_SPACE,
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
        ],
        bump
    )]
    pub position: Account<'info, Position>,

    /// Vault token account (PDA) to hold staked LP tokens for this position
    #[account(
        init,
        payer = owner,
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
            VAULT_SEED,
        ],
        bump,
        token::mint = lp_mint,
        token::authority = global_state,
    )]
    pub position_vault: InterfaceAccount<'info, TokenAccount>,

    /// The GT Store account (mutated by CPI)
    #[account(mut)]
    pub gt_store: AccountLoader<'info, Store>,

    /// GT program
    pub gt_program: Program<'info, GmsolStore>,

    /// Owner paying rent and recorded as position owner
    #[account(mut)]
    pub owner: Signer<'info>,

    /// User's LP token account (must match lp_mint and owner)
    #[account(
        mut,
        constraint = user_lp_token.mint == lp_mint.key(),
        constraint = user_lp_token.owner == owner.key(),
    )]
    pub user_lp_token: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// Accounts context for calculating GT reward from a Position
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct CalculateGtReward<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Account<'info, GlobalState>,
    /// The GT Store account (loaded & mutated by CPI)
    #[account(mut)]
    pub gt_store: AccountLoader<'info, Store>,
    /// The GT program
    pub gt_program: Program<'info, GmsolStore>,
    /// Position tied to (global_state, owner, position_id)
    #[account(
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
        ],
        bump = position.bump,
        has_one = owner,
        has_one = global_state
    )]
    pub position: Account<'info, Position>,
    /// Owner of the position (not required to sign for read-only calc)
    /// CHECK: only used for PDA seeds match via has_one
    pub owner: UncheckedAccount<'info>,
}

/// Accounts context for claiming GT reward and updating Position snapshot
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct ClaimGt<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Account<'info, GlobalState>,

    /// The GT Store account (mutated by CPI)
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,

    /// The GT program
    pub gt_program: Program<'info, GmsolStore>,

    /// Position tied to (global_state, owner, position_id)
    #[account(
        mut,
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
        ],
        bump = position.bump,
        has_one = owner,
        has_one = global_state
    )]
    pub position: Account<'info, Position>,

    /// Owner of the position
    pub owner: Signer<'info>,

    /// GT User account (mut) managed by the GT program; must correspond to (store, owner)
    #[account(
        mut,
        has_one = owner,
        has_one = store,
    )]
    pub gt_user: AccountLoader<'info, UserHeader>,

    /// CHECK: GT program's event authority PDA required by #[event_cpi] calls
    pub event_authority: UncheckedAccount<'info>,
}

/// Accounts context for unstaking LP; combines claim + partial/full exit
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct UnstakeLp<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Account<'info, GlobalState>,

    /// LP token mint for this position (must match position.lp_mint)
    pub lp_mint: InterfaceAccount<'info, Mint>,

    /// The GT Store account (mutated by CPI)
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,

    /// The GT program
    pub gt_program: Program<'info, GmsolStore>,

    /// Position tied to (global_state, owner, position_id)
    #[account(
        mut,
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
        ],
        bump = position.bump,
        has_one = owner,
        has_one = global_state
    )]
    pub position: Account<'info, Position>,

    /// Vault holding staked LP tokens (PDA)
    #[account(
        mut,
        seeds = [
            POSITION_SEED,
            global_state.key().as_ref(),
            owner.key().as_ref(),
            &position_id.to_le_bytes(),
            VAULT_SEED,
        ],
        bump,
        token::mint = lp_mint,
        token::authority = global_state,
    )]
    pub position_vault: InterfaceAccount<'info, TokenAccount>,

    /// Owner of the position
    pub owner: Signer<'info>,

    /// GT User account (mut) managed by the GT program; must correspond to (store, owner)
    #[account(
        mut,
        has_one = owner,
        has_one = store,
    )]
    pub gt_user: AccountLoader<'info, UserHeader>,

    /// Destination LP token account to receive unstaked tokens
    #[account(
        mut,
        constraint = user_lp_token.mint == lp_mint.key(),
        constraint = user_lp_token.owner == owner.key(),
    )]
    pub user_lp_token: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: GT program's event authority PDA required by #[event_cpi] calls
    pub event_authority: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct UpdateMinStakeValue<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateApyGradient<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferAuthority<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority proposing a transfer
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    /// Global config (PDA). The signer must equal `global_state.pending_authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = pending_authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Pending authority accepting control (must match `global_state.pending_authority`)
    pub pending_authority: Signer<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct GlobalState {
    /// Program administrator with governance privileges
    pub authority: Pubkey,
    /// Pending authority awaiting acceptance (Pubkey::default() if none)
    pub pending_authority: Pubkey,
    /// GT token mint address
    pub gt_mint: Pubkey,
    /// APY gradient buckets (53), each is 1e20-scaled APR for week buckets [0-1), [1-2), ..., [52, +inf)
    pub apy_gradient: [u128; 53],
    /// LP token price in USD scaled by 1e20
    pub lp_token_price: u128,
    /// Minimum stake value in USD scaled by 1e20
    pub min_stake_value: u128,
    /// PDA bump for this GlobalState (derived from seed [GLOBAL_STATE_SEED])
    pub bump: u8,
}

/// Position account to persist LP stake data and snapshot stake-time values
#[account]
#[derive(InitSpace)]
pub struct Position {
    /// Owner of this LP position
    pub owner: Pubkey,
    /// Ties position to a specific GlobalState
    pub global_state: Pubkey,
    /// LP token mint for this position
    pub lp_mint: Pubkey,
    /// PDA token account that escrows staked LP tokens
    pub vault: Pubkey,
    /// Position id to allow multiple positions per owner
    pub position_id: u64,
    /// Staked LP amount at stake time (raw amount as provided by caller; optional semantics)
    pub staked_amount: u64,
    /// Staked value in USD (scaled by 1e20) captured at stake time
    pub staked_value_usd: u128,
    /// Stake start unix timestamp (seconds)
    pub stake_start_time: i64,
    /// Cumulative inverse-cost factor snapshot (last claim/stake checkpoint)
    pub cum_inv_cost: u128,
    /// PDA bump
    pub bump: u8,
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
