use crate::error::CompetitionError;
use crate::state::competition::{Competition, LeaderEntry, Participant};
use anchor_lang::prelude::*;

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
    require_keys_eq!(
        ctx.accounts.store_program.key(),
        c.store_program,
        CompetitionError::InvalidCaller
    );
    require!(
        ctx.accounts.store_program.executable,
        CompetitionError::InvalidCaller
    );

    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= c.start_time && now <= c.end_time,
        CompetitionError::OutsideCompetitionTime
    );

    let p = &mut ctx.accounts.participant;
    // If this is a freshly created Participant, populate static fields
    if p.owner == Pubkey::default() {
        p.owner = ctx.accounts.trader.key();
        p.competition = c.key();
    }

    // Safety check: the PDA must belong to the same trader
    require_keys_eq!(
        p.owner,
        ctx.accounts.trader.key(),
        CompetitionError::InvalidCaller
    );

    p.volume += volume;
    p.last_updated_at = now;

    // ------------- update in‑account leaderboard -------------
    let lb = &mut c.leaderboard;

    // (a) if trader already in leaderboard, update volume
    if let Some(entry) = lb.iter_mut().find(|e| e.address == p.owner) {
        entry.volume = p.volume;
    } else {
        // (b) otherwise push new entry then sort
        lb.push(LeaderEntry {
            address: p.owner,
            volume: p.volume,
        });
    }

    // sort desc by volume and keep only top‑5
    lb.sort_by(|a, b| b.volume.cmp(&a.volume));
    if lb.len() > 5 {
        lb.truncate(5);
    }

    Ok(())
}
