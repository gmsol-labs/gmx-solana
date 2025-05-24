use crate::{
    error::CompetitionError,
    states::{Competition, COMPETITION_SEED},
};
use anchor_lang::prelude::*;

/// Initialize the [`Competition`] account.
///
/// Must be invoked by a keeper before the trading window starts.
#[derive(Accounts)]
#[instruction(start_time: i64)]
pub struct InitializeCompetition<'info> {
    /// Payer and the authority of the competition.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The global competition PDA.
    #[account(
        init,
        payer = payer,
        seeds = [
            COMPETITION_SEED,
            payer.key.as_ref(),
            &start_time.to_le_bytes(),
        ],
        bump,
        space = 8 + Competition::INIT_SPACE,
    )]
    pub competition: Account<'info, Competition>,
    pub system_program: Program<'info, System>,
    /// The store program id.
    /// # CHECK: only the address is used.
    pub store_program: UncheckedAccount<'info>,
}

impl InitializeCompetition<'_> {
    pub(crate) fn invoke(
        ctx: Context<Self>,
        start_time: i64,
        end_time: i64,
        volume_threshold: u128,
        time_extension: i64,
        max_extension: i64,
    ) -> Result<()> {
        require!(start_time < end_time, CompetitionError::InvalidTimeRange);
        require!(time_extension > 0, CompetitionError::InvalidTimeExtension);
        require!(
            volume_threshold > 0,
            CompetitionError::InvalidVolumeThreshold
        );
        require!(max_extension > 0, CompetitionError::InvalidMaxExtension);
        require!(
            max_extension >= time_extension,
            CompetitionError::InvalidMaxExtension
        );

        let comp = &mut ctx.accounts.competition;
        comp.bump = ctx.bumps.competition;
        comp.authority = ctx.accounts.payer.key();
        comp.start_time = start_time;
        comp.end_time = end_time;
        comp.is_active = true;
        comp.store_program = ctx.accounts.store_program.key();
        comp.leaderboard = Vec::default();
        comp.volume_threshold = volume_threshold;
        comp.time_extension = time_extension;
        comp.max_extension = max_extension;
        comp.extension_trigger = None;
        Ok(())
    }
}
