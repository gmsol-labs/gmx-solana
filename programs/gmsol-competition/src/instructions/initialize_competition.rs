use crate::state::competition::Competition;
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeCompetition<'info> {
    #[account(init, payer = authority, space = 8 + Competition::LEN)]
    pub competition: Account<'info, Competition>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn init_competition_handler(
    ctx: Context<InitializeCompetition>,
    start_time: i64,
    end_time: i64,
    store_program: Pubkey,
) -> Result<()> {
    let comp = &mut ctx.accounts.competition;
    comp.authority = ctx.accounts.authority.key();
    comp.start_time = start_time;
    comp.end_time = end_time;
    comp.is_active = true;
    comp.store_program = store_program;
    comp.leaderboard = Vec::new();
    Ok(())
}
