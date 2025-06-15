use anchor_lang::prelude::*;

use crate::{
    states::{ActionStats, Config, ACTION_STATS_SEED, CALLER_PROGRAM_ID},
    CALLBACK_AUTHORITY_SEED,
};

/// Callback accounts.
#[derive(Accounts)]
#[instruction(authority_bump: u8, action_kind: u8)]
pub struct OnCallback<'info> {
    /// The callback authority.
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = authority_bump,
        seeds::program = CALLER_PROGRAM_ID,
    )]
    pub authority: Signer<'info>,
    /// The config account.
    #[account(mut)]
    pub config: Account<'info, Config>,
    /// the action stats account.
    #[account(
        mut,
        seeds = [
            ACTION_STATS_SEED,
            owner.key.as_ref(),
            &[action_kind],
        ],
        bump = action_stats.bump,
        constraint = action_stats.initialized,
        constraint = action_stats.action_kind == action_kind,
        has_one = owner,
    )]
    pub action_stats: Account<'info, ActionStats>,
    /// The owner of the action account.
    /// CHECK: This account is unchecked because only its address is used.
    pub owner: UncheckedAccount<'info>,
    /// The action account.
    /// CHECK: This account is unchecked because only its address is used.
    pub action: UncheckedAccount<'info>,
}

impl OnCallback<'_> {
    pub(crate) fn invoke(
        trigger: On,
        ctx: Context<Self>,
        _authority_bump: u8,
        _action_kind: u8,
        _callback_version: u8,
        success: bool,
        extra_account_count: u8,
    ) -> Result<()> {
        debug_assert!(ctx.remaining_accounts.len() >= usize::from(extra_account_count));
        ctx.accounts.config.calls += 1;
        match trigger {
            On::Created => ctx.accounts.handle_created(success),
            On::Updated => ctx.accounts.handle_updated(success),
            On::Executed => ctx.accounts.handle_executed(success),
            On::Closed => ctx.accounts.handle_closed(success),
        }
    }

    fn handle_created(&mut self, success: bool) -> Result<()> {
        self.action_stats.total_created += 1;
        self.action_stats.last_created_at = Clock::get()?.unix_timestamp;

        msg!(
            "{}: on_created, success={} calls={}",
            self.config.prefix,
            success,
            self.config.calls
        );
        Ok(())
    }

    fn handle_updated(&mut self, success: bool) -> Result<()> {
        self.action_stats.update_count += 1;
        self.action_stats.last_updated_at = Clock::get()?.unix_timestamp;

        msg!(
            "{}: on_updated, success={} calls={}",
            self.config.prefix,
            success,
            self.config.calls
        );
        Ok(())
    }

    fn handle_executed(&mut self, success: bool) -> Result<()> {
        self.action_stats.total_executed += 1;
        self.action_stats.last_executed_at = Clock::get()?.unix_timestamp;

        msg!(
            "{}: on_executed, success={} calls={}",
            self.config.prefix,
            success,
            self.config.calls
        );
        Ok(())
    }

    fn handle_closed(&mut self, success: bool) -> Result<()> {
        self.action_stats.total_closed += 1;
        self.action_stats.last_closed_at = Clock::get()?.unix_timestamp;

        msg!(
            "{}: on_closed, success={} calls={}",
            self.config.prefix,
            success,
            self.config.calls
        );
        Ok(())
    }
}

/// Callback trigger.
pub(crate) enum On {
    /// On created.
    Created,
    /// On updated.
    Updated,
    /// On executed.
    Executed,
    /// On closed.
    Closed,
}
