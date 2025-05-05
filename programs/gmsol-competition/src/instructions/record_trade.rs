use anchor_lang::prelude::*;
use crate::state::competition::{Competition, Participant};
use crate::error::CompetitionError;

#[derive(Accounts)]
pub struct RecordTrade<'info> {
    #[account(mut)]
    pub competition: Account<'info, Competition>,

    #[account(
        init_if_needed,
        payer = payer,
        seeds = [b"participant", competition.key().as_ref(), trader.key().as_ref()],
        bump,
        space = 8 + Participant::LEN
    )]
    pub participant: Account<'info, Participant>,

    /// CHECK: must be executable store program
    pub store_program: UncheckedAccount<'info>,
    /// CHECK: trader pubkey
    pub trader: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn record_trade_handler(ctx: Context<RecordTrade>, volume: u64) -> Result<()> {
    let c = &mut ctx.accounts.competition;
    require!(c.is_active, CompetitionError::CompetitionNotActive);
    require_keys_eq!(ctx.accounts.store_program.key(), c.store_program, CompetitionError::InvalidCaller);
    require!(ctx.accounts.store_program.executable, CompetitionError::InvalidCaller);

    let now = Clock::get()?.unix_timestamp;
    require!(now >= c.start_time && now <= c.end_time, CompetitionError::OutsideCompetitionTime);

    let p = &mut ctx.accounts.participant;
    p.volume += volume;
    p.last_updated_at = now;
    Ok(())
}