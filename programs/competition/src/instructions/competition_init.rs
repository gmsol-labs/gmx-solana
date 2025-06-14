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
}

impl InitializeCompetition<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn invoke(
        ctx: Context<Self>,
        start_time: i64,
        end_time: i64,
        volume_threshold: u128,
        extension_duration: i64,
        extension_cap: i64,
        only_count_increase: bool,
        volume_merge_window: i64,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        require!(start_time > now, CompetitionError::InvalidTimeRange);
        require!(start_time < end_time, CompetitionError::InvalidTimeRange);
        require!(
            extension_duration > 0,
            CompetitionError::InvalidTimeExtension
        );
        require!(
            volume_threshold > 0,
            CompetitionError::InvalidVolumeThreshold
        );
        require!(extension_cap > 0, CompetitionError::InvalidMaxExtension);
        require!(
            extension_cap >= extension_duration,
            CompetitionError::InvalidMaxExtension
        );
        require!(
            volume_merge_window > 0,
            CompetitionError::InvalidVolumeMergeWindow
        );

        let comp = &mut ctx.accounts.competition;
        comp.bump = ctx.bumps.competition;
        comp.authority = ctx.accounts.payer.key();
        comp.start_time = start_time;
        comp.end_time = end_time;
        comp.leaderboard = Vec::default();
        comp.volume_threshold = volume_threshold;
        comp.extension_duration = extension_duration;
        comp.extension_cap = extension_cap;
        comp.extension_triggerer = None;
        comp.only_count_increase = only_count_increase;
        comp.volume_merge_window = volume_merge_window;
        Ok(())
    }
}
