use anchor_lang::prelude::AccountsClose;
use anchor_lang::prelude::*;
use anchor_spl::token_interface as token_if;
use anchor_spl::token_interface::{
    CloseAccount, Mint, TokenAccount, TokenInterface, TransferChecked,
};
use gmsol_model::num::MulDiv;
use gmsol_model::utils::apply_factor;
use gmsol_programs::gmsol_store::constants::{MARKET_DECIMALS, MARKET_USD_UNIT};
use gmsol_programs::gmsol_store::{
    accounts::{Glv, Market, Oracle, Store, TokenMapHeader, UserHeader},
    cpi as gt_cpi,
    cpi::accounts::{
        GetGlvTokenValue, GetMarketTokenValue, MintGtReward as GtMintCtx,
        UpdateGtCumulativeInvCostFactor as GtUpdateCtx,
    },
    cpi::Return as GtReturn,
    cpi::{get_glv_token_value, get_market_token_value},
    program::GmsolStore,
};

#[constant]
pub const POSITION_SEED: &'static [u8] = b"position";
#[constant]
pub const GLOBAL_STATE_SEED: &'static [u8] = b"global_state";
#[constant]
pub const VAULT_SEED: &'static [u8] = b"vault";
#[constant]
pub const DEFAULT_PRICING_STALENESS_SECONDS: u32 = 300; // Default 5 minutes
                                                        // IDL-safe constants (u8) exposed via #[constant]
#[constant]
pub const APY_BUCKETS_U8: u8 = 53;
#[constant]
pub const APY_LAST_INDEX_U8: u8 = APY_BUCKETS_U8 - 1; // 52

// Internal mirrors as usize for array lengths and indexing
pub const APY_BUCKETS: usize = APY_BUCKETS_U8 as usize;
pub const APY_LAST_INDEX: usize = APY_LAST_INDEX_U8 as usize;
#[constant]
pub const APY_MAX: u128 = 200_000_000_000_000_000_000u128; // 200% at 1e20 scale

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
        initial_apy: u128, // Initial APY for all buckets (1e20-scaled)
    ) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        global_state.authority = ctx.accounts.authority.key();
        global_state.pending_authority = Pubkey::default();
        global_state.gt_mint = ctx.accounts.gt_mint.key();

        // Cap-check and initialize all buckets with the same initial APY
        require!(initial_apy <= APY_MAX, ErrorCode::ApyTooLarge);
        global_state.apy_gradient = [initial_apy; APY_BUCKETS];

        global_state.lp_token_price = MARKET_USD_UNIT; // $1.00 in 1e20 units
        global_state.min_stake_value = min_stake_value;
        global_state.claim_enabled = false;
        global_state.pricing_staleness_seconds = DEFAULT_PRICING_STALENESS_SECONDS;
        global_state.bump = ctx.bumps.global_state;
        msg!(
            "LP staking program initialized, min_stake_value(1e20)={}, initial_apy(1e20)={}, pricing_staleness={}s",
            min_stake_value,
            initial_apy,
            global_state.pricing_staleness_seconds
        );
        Ok(())
    }

    /// Toggle whether LPs can claim GT without unstaking.
    pub fn set_claim_enabled(ctx: Context<SetClaimEnabled>, enabled: bool) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        gs.claim_enabled = enabled;
        msg!("claim_enabled set to {}", enabled);
        Ok(())
    }

    /// Set pricing staleness configuration
    pub fn set_pricing_staleness(
        ctx: Context<SetPricingStaleness>,
        staleness_seconds: u32,
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;
        gs.pricing_staleness_seconds = staleness_seconds;
        msg!("pricing staleness set to {} seconds", staleness_seconds);
        Ok(())
    }

    /// Update APY gradient with a sparse table (only non-zero buckets)
    pub fn update_apy_gradient_sparse(
        ctx: Context<UpdateApyGradient>,
        bucket_indices: Vec<u8>, // indices of buckets to update
        apy_values: Vec<u128>,   // corresponding APY values (1e20-scaled)
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;

        // Lengths must match
        require!(
            bucket_indices.len() == apy_values.len(),
            ErrorCode::InvalidArgument
        );

        // Apply sparse updates
        for (idx, val) in bucket_indices.into_iter().zip(apy_values.into_iter()) {
            require!((idx as usize) < APY_BUCKETS, ErrorCode::InvalidArgument);
            require!(val <= APY_MAX, ErrorCode::ApyTooLarge);
            gs.apy_gradient[idx as usize] = val;
        }

        msg!(
            "APY gradient updated via sparse entries (total buckets = {})",
            gs.apy_gradient.len()
        );
        Ok(())
    }

    /// Update APY gradient for a contiguous range of buckets
    pub fn update_apy_gradient_range(
        ctx: Context<UpdateApyGradient>,
        start_bucket: u8,
        end_bucket: u8,
        apy_values: Vec<u128>, // Must match the range size
    ) -> Result<()> {
        let gs = &mut ctx.accounts.global_state;

        require!(
            (start_bucket as usize) < APY_BUCKETS && (end_bucket as usize) < APY_BUCKETS,
            ErrorCode::InvalidArgument
        );
        require!(start_bucket <= end_bucket, ErrorCode::InvalidArgument);

        let expected_size = (end_bucket - start_bucket + 1) as usize;
        require!(
            apy_values.len() == expected_size,
            ErrorCode::InvalidArgument
        );

        // Apply range updates
        for (i, apy_value) in apy_values.into_iter().enumerate() {
            require!(apy_value <= APY_MAX, ErrorCode::ApyTooLarge);
            let bucket_idx = start_bucket as usize + i;
            gs.apy_gradient[bucket_idx] = apy_value;
        }

        msg!(
            "APY gradient updated for buckets {}..={} ({} values)",
            start_bucket,
            end_bucket,
            expected_size
        );
        Ok(())
    }

    /// Create a new LP position with manual pricing (for generic LP tokens)
    pub fn stake_lp<'info>(
        ctx: Context<'_, '_, 'info, 'info, StakeLp<'info>>,
        position_id: u64,
        lp_staked_amount: u64,
        lp_staked_value: u128, // Manual scaled USD value (no longer optional)
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;

        // Enforce non-zero staked amount
        require!(lp_staked_amount > 0, ErrorCode::InvalidArgument);

        // Use the provided manual staked value
        let actual_staked_value = lp_staked_value;

        // Enforce minimum stake value (scaled 1e20)
        require!(
            actual_staked_value >= ctx.accounts.global_state.min_stake_value,
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
        msg!("About to call update_gt_cumulative_inv_cost_factor");
        let r: GtReturn<u128> = gt_cpi::update_gt_cumulative_inv_cost_factor(cpi_ctx)?;
        let c_start: u128 = r.get();

        // Transfer LP tokens from user to the position vault
        let cpi_accounts = TransferChecked {
            from: ctx.accounts.user_lp_token.to_account_info(),
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.position_vault.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token_if::transfer_checked(cpi_ctx, lp_staked_amount, ctx.accounts.lp_mint.decimals)?;

        // Init position fields
        let position = &mut ctx.accounts.position;
        position.owner = ctx.accounts.owner.key();
        position.global_state = ctx.accounts.global_state.key();
        position.lp_mint = ctx.accounts.lp_mint.key();
        position.vault = ctx.accounts.position_vault.key();
        position.position_id = position_id;
        position.staked_amount = lp_staked_amount;
        position.staked_value_usd = actual_staked_value;
        position.stake_start_time = now;
        position.cum_inv_cost = c_start;
        position.bump = ctx.bumps.position;

        msg!(
            "Stake created: owner={}, amount={}, value(1e20)={}, start_ts={}, C_start={}, pos_id={}",
            position.owner,
            lp_staked_amount,
            actual_staked_value,
            now,
            c_start,
            position_id
        );
        Ok(())
    }

    /// Stake GM tokens with automatic pricing via CPI
    pub fn stake_gm<'info>(
        ctx: Context<'_, '_, 'info, 'info, StakeGm<'info>>,
        position_id: u64,
        gm_staked_amount: u64,
    ) -> Result<()> {
        // Get GM token value using pricing CPI
        let gm_value = get_gm_token_value_via_cpi(
            &ctx.accounts.global_state,
            &ctx.accounts.pricing_store,
            &ctx.accounts.token_map,
            &ctx.accounts.oracle,
            &ctx.accounts.market,
            &ctx.accounts.market_token,
            &ctx.accounts.gt_program,
            ctx.remaining_accounts,
            gm_staked_amount,
        )?;

        // Call shared stake logic
        execute_stake(
            &ctx.accounts.global_state,
            &ctx.accounts.lp_mint,
            &mut ctx.accounts.position,
            &ctx.accounts.position_vault,
            &ctx.accounts.gt_store,
            &ctx.accounts.gt_program,
            &ctx.accounts.owner,
            &ctx.accounts.user_lp_token,
            &ctx.accounts.token_program,
            ctx.bumps.position,
            position_id,
            gm_staked_amount,
            gm_value,
        )?;

        msg!(
            "GM tokens staked successfully: amount={}, value={}",
            gm_staked_amount,
            gm_value
        );
        Ok(())
    }

    /// Stake GLV tokens with automatic pricing via CPI
    pub fn stake_glv<'info>(
        ctx: Context<'_, '_, 'info, 'info, StakeGlv<'info>>,
        position_id: u64,
        glv_staked_amount: u64,
    ) -> Result<()> {
        // Get GLV token value using pricing CPI
        let glv_value = get_glv_token_value_via_cpi(
            &ctx.accounts.global_state,
            &ctx.accounts.pricing_store,
            &ctx.accounts.token_map,
            &ctx.accounts.oracle,
            &ctx.accounts.glv,
            &ctx.accounts.glv_token,
            &ctx.accounts.gt_program,
            ctx.remaining_accounts,
            glv_staked_amount,
        )?;

        // Call shared stake logic
        execute_stake(
            &ctx.accounts.global_state,
            &ctx.accounts.lp_mint,
            &mut ctx.accounts.position,
            &ctx.accounts.position_vault,
            &ctx.accounts.gt_store,
            &ctx.accounts.gt_program,
            &ctx.accounts.owner,
            &ctx.accounts.user_lp_token,
            &ctx.accounts.token_program,
            ctx.bumps.position,
            position_id,
            glv_staked_amount,
            glv_value,
        )?;

        msg!(
            "GLV tokens staked successfully: amount={}, value={}",
            glv_staked_amount,
            glv_value
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
        // Disallow free claims unless explicitly enabled by authority
        require!(global_state.claim_enabled, ErrorCode::ClaimDisabled);

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

/// Compute time-weighted average APR over [start, now] using APY_BUCKETS-bucket weekly gradient (1e20-scaled).
fn compute_time_weighted_apy(
    stake_start_time: i64,
    now: i64,
    apy_gradient: &[u128; APY_BUCKETS],
) -> u128 {
    if now <= stake_start_time {
        return apy_gradient[0];
    }
    let total_seconds: u128 = (now - stake_start_time) as u128;
    if total_seconds == 0 {
        return apy_gradient[0];
    }

    let full_weeks: u128 = total_seconds / SECONDS_PER_WEEK;
    let rem_seconds: u128 = total_seconds % SECONDS_PER_WEEK;

    // Sum full-week contributions
    let mut acc: u128 = 0;
    let capped_full: u128 = full_weeks.min(APY_LAST_INDEX as u128);
    for i in 0..(capped_full as usize) {
        acc = acc.saturating_add(apy_gradient[i].saturating_mul(SECONDS_PER_WEEK));
    }
    if full_weeks > (APY_LAST_INDEX as u128) {
        let extra = full_weeks - (APY_LAST_INDEX as u128); // weeks APY_LAST_INDEX+ use bucket APY_LAST_INDEX
        acc = acc.saturating_add(
            apy_gradient[APY_LAST_INDEX].saturating_mul(SECONDS_PER_WEEK.saturating_mul(extra)),
        );
    }

    // Add partial-week remainder
    if rem_seconds > 0 {
        let idx = usize::try_from(full_weeks.min(APY_LAST_INDEX as u128)).unwrap_or(APY_LAST_INDEX);
        acc = acc.saturating_add(apy_gradient[idx].saturating_mul(rem_seconds));
    }

    acc / total_seconds
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
    pub global_state: Box<Account<'info, GlobalState>>,

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
    pub position: Box<Account<'info, Position>>,

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

/// Accounts context for staking GM tokens with automatic pricing
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct StakeGm<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Box<Account<'info, GlobalState>>,

    /// GM token mint to be staked (market token)
    pub lp_mint: InterfaceAccount<'info, Mint>,

    /// Position PDA to initialize
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
    pub position: Box<Account<'info, Position>>,

    /// Vault token account (PDA) to hold staked GM tokens
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

    /// User's GM token account
    #[account(
        mut,
        constraint = user_lp_token.mint == lp_mint.key(),
        constraint = user_lp_token.owner == owner.key(),
    )]
    pub user_lp_token: InterfaceAccount<'info, TokenAccount>,

    /// Pricing accounts for GM token valuation
    pub pricing_store: AccountLoader<'info, Store>,
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    #[account(mut)]
    pub oracle: AccountLoader<'info, Oracle>,
    pub market: AccountLoader<'info, Market>,
    pub market_token: InterfaceAccount<'info, Mint>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// Accounts context for staking GLV tokens with automatic pricing
#[derive(Accounts)]
#[instruction(position_id: u64)]
pub struct StakeGlv<'info> {
    /// Global config (PDA)
    #[account(seeds = [GLOBAL_STATE_SEED], bump = global_state.bump)]
    pub global_state: Box<Account<'info, GlobalState>>,

    /// GLV token mint to be staked
    pub lp_mint: InterfaceAccount<'info, Mint>,

    /// Position PDA to initialize
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
    pub position: Box<Account<'info, Position>>,

    /// Vault token account (PDA) to hold staked GLV tokens
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

    /// User's GLV token account
    #[account(
        mut,
        constraint = user_lp_token.mint == lp_mint.key(),
        constraint = user_lp_token.owner == owner.key(),
    )]
    pub user_lp_token: InterfaceAccount<'info, TokenAccount>,

    /// Pricing accounts for GLV token valuation
    pub pricing_store: AccountLoader<'info, Store>,
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    #[account(mut)]
    pub oracle: AccountLoader<'info, Oracle>,
    pub glv: AccountLoader<'info, Glv>,
    pub glv_token: InterfaceAccount<'info, Mint>,

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
pub struct SetClaimEnabled<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateMinStakeValue<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority
    pub authority: Signer<'info>,
}

/// Accounts for APY gradient updates (used by sparse and range initializers)
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

#[derive(Accounts)]
pub struct SetPricingStaleness<'info> {
    /// Global config (PDA). The `authority` signer must match `global_state.authority`.
    #[account(mut, seeds = [GLOBAL_STATE_SEED], bump = global_state.bump, has_one = authority)]
    pub global_state: Account<'info, GlobalState>,
    /// Current authority
    pub authority: Signer<'info>,
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
    /// APY gradient buckets (APY_BUCKETS), each is 1e20-scaled APR for week buckets [0-1), [1-2), ..., [APY_BUCKETS, +inf)
    pub apy_gradient: [u128; APY_BUCKETS],
    /// LP token price in USD scaled by 1e20
    pub lp_token_price: u128,
    /// Minimum stake value in USD scaled by 1e20
    pub min_stake_value: u128,
    /// If true, LPs may call `claim_gt` at any time without unstaking
    pub claim_enabled: bool,
    /// PDA bump for this GlobalState (derived from seed [GLOBAL_STATE_SEED])
    pub bump: u8,
    /// Price staleness configuration in seconds
    pub pricing_staleness_seconds: u32,
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
    #[msg("APY value exceeds the configured maximum")]
    ApyTooLarge,
    #[msg("Claim is disabled by protocol policy")]
    ClaimDisabled,
}

/// Shared core stake logic for all stake types
fn execute_stake<'info>(
    global_state: &Account<'info, GlobalState>,
    lp_mint: &InterfaceAccount<'info, Mint>,
    position: &mut Account<'info, Position>,
    position_vault: &InterfaceAccount<'info, TokenAccount>,
    gt_store: &AccountLoader<'info, Store>,
    gt_program: &Program<'info, GmsolStore>,
    owner: &Signer<'info>,
    user_lp_token: &InterfaceAccount<'info, TokenAccount>,
    token_program: &Interface<'info, TokenInterface>,
    position_bump: u8,
    position_id: u64,
    staked_amount: u64,
    staked_value: u128,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    // Enforce non-zero staked amount
    require!(staked_amount > 0, ErrorCode::InvalidArgument);

    // Enforce minimum stake value
    require!(
        staked_value >= global_state.min_stake_value,
        ErrorCode::InvalidArgument
    );

    // Get C(start) snapshot via CPI
    let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
    let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

    let cpi_ctx = CpiContext::new_with_signer(
        gt_program.to_account_info(),
        GtUpdateCtx {
            authority: global_state.to_account_info(),
            store: gt_store.to_account_info(),
        },
        signer_seeds,
    );
    let r: GtReturn<u128> = gt_cpi::update_gt_cumulative_inv_cost_factor(cpi_ctx)?;
    let c_start: u128 = r.get();

    // Transfer LP tokens from user to vault
    let cpi_accounts = TransferChecked {
        from: user_lp_token.to_account_info(),
        mint: lp_mint.to_account_info(),
        to: position_vault.to_account_info(),
        authority: owner.to_account_info(),
    };
    let cpi_ctx = CpiContext::new(token_program.to_account_info(), cpi_accounts);
    token_if::transfer_checked(cpi_ctx, staked_amount, lp_mint.decimals)?;

    // Initialize position
    position.owner = owner.key();
    position.global_state = global_state.key();
    position.lp_mint = lp_mint.key();
    position.vault = position_vault.key();
    position.position_id = position_id;
    position.staked_amount = staked_amount;
    position.staked_value_usd = staked_value;
    position.stake_start_time = now;
    position.cum_inv_cost = c_start;
    position.bump = position_bump;

    msg!(
        "Stake executed: owner={}, amount={}, value(1e20)={}, start_ts={}, C_start={}, pos_id={}",
        position.owner,
        staked_amount,
        staked_value,
        now,
        c_start,
        position_id
    );

    Ok(())
}

/// Get GM token value via CPI to gmsol_store
fn get_gm_token_value_via_cpi<'info>(
    global_state: &Account<'info, GlobalState>,
    pricing_store: &AccountLoader<'info, Store>,
    token_map: &AccountLoader<'info, TokenMapHeader>,
    oracle: &AccountLoader<'info, Oracle>,
    market: &AccountLoader<'info, Market>,
    market_token: &InterfaceAccount<'info, Mint>,
    gt_program: &Program<'info, GmsolStore>,
    remaining_accounts: &'info [AccountInfo<'info>],
    amount: u64,
) -> Result<u128> {
    let cpi_accounts = GetMarketTokenValue {
        authority: global_state.to_account_info(),
        store: pricing_store.to_account_info(),
        token_map: token_map.to_account_info(),
        oracle: oracle.to_account_info(),
        market: market.to_account_info(),
        market_token: market_token.to_account_info(),
        event_authority: global_state.to_account_info(),
        program: gt_program.to_account_info(),
    };

    // Use GlobalState PDA as signer for the CPI call
    let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
    let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

    let cpi_ctx =
        CpiContext::new_with_signer(gt_program.to_account_info(), cpi_accounts, signer_seeds)
            .with_remaining_accounts(remaining_accounts.to_vec());

    let result = get_market_token_value(
        cpi_ctx,
        amount,
        "max_after_deposit".to_string(), // Conservative pricing
        false,                           // maximize: false
        global_state.pricing_staleness_seconds,
        false, // emit_event: false
    )?;

    Ok(result.get())
}

/// Get GLV token value via CPI to gmsol_store
fn get_glv_token_value_via_cpi<'info>(
    global_state: &Account<'info, GlobalState>,
    pricing_store: &AccountLoader<'info, Store>,
    token_map: &AccountLoader<'info, TokenMapHeader>,
    oracle: &AccountLoader<'info, Oracle>,
    glv: &AccountLoader<'info, Glv>,
    glv_token: &InterfaceAccount<'info, Mint>,
    gt_program: &Program<'info, GmsolStore>,
    remaining_accounts: &'info [AccountInfo<'info>],
    amount: u64,
) -> Result<u128> {
    let cpi_accounts = GetGlvTokenValue {
        authority: global_state.to_account_info(),
        store: pricing_store.to_account_info(),
        token_map: token_map.to_account_info(),
        oracle: oracle.to_account_info(),
        glv: glv.to_account_info(),
        glv_token: glv_token.to_account_info(),
        event_authority: global_state.to_account_info(),
        program: gt_program.to_account_info(),
    };

    // Use GlobalState PDA as signer for the CPI call
    let gs_seeds: &[&[u8]] = &[GLOBAL_STATE_SEED, &[global_state.bump]];
    let signer_seeds: &[&[&[u8]]] = &[gs_seeds];

    let cpi_ctx =
        CpiContext::new_with_signer(gt_program.to_account_info(), cpi_accounts, signer_seeds)
            .with_remaining_accounts(remaining_accounts.to_vec());

    let result = get_glv_token_value(
        cpi_ctx,
        amount,
        false, // maximize: false for conservative pricing
        global_state.pricing_staleness_seconds,
        false, // emit_event: false
    )?;

    Ok(result.get())
}
