use anchor_lang::prelude::*;

use crate::states::{ActionStats, ACTION_STATS_SEED};

/// Create [`ActionStats`] account idempotently.
#[derive(Accounts)]
#[instruction(action_kind: u8)]
pub struct CreateActionStatsIdempotent<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The [`ActionStats`] account.
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + ActionStats::INIT_SPACE,
        seeds = [
            ACTION_STATS_SEED,
            owner.key.as_ref(),
            &[action_kind],
        ],
        bump
    )]
    pub action_stats: Account<'info, ActionStats>,
    /// The owner of the action account.
    /// CHECK: This account is unchecked because only its address is used.
    pub owner: UncheckedAccount<'info>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

impl CreateActionStatsIdempotent<'_> {
    pub(crate) fn invoke(ctx: Context<Self>, action_kind: u8) -> Result<()> {
        let action_stats = &mut ctx.accounts.action_stats;
        if !action_stats.initialized {
            action_stats.initialized = true;
            action_stats.bump = ctx.bumps.action_stats;
            action_stats.action_kind = action_kind;
            action_stats.owner = ctx.accounts.owner.key();
        }
        Ok(())
    }
}
