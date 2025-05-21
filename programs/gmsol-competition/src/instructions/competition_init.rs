use crate::{
    error::CompetitionError,
    states::{Competition, COMPETITION_SEED, MAX_LEADERBOARD_LEN},
};
use anchor_lang::prelude::*;

/// Initialize the [`Competition`] account.
///
/// Must be invoked by a keeper before the trading window starts.
#[derive(Accounts)]
pub struct InitializeCompetition<'info> {
    /// Payer and keeper.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The global competition PDA.
    #[account(
        init,
        payer  = payer,
        seeds  = [COMPETITION_SEED],
        bump,
        space  = 8 + Competition::INIT_SPACE,
    )]
    pub competition: Account<'info, Competition>,
    pub system_program: Program<'info, System>,
}

impl InitializeCompetition<'_> {
    pub(crate) fn invoke(
        ctx: Context<Self>,
        start_time: i64,
        end_time: i64,
        store_program: Pubkey,
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
        comp.authority = ctx.accounts.payer.key();
        comp.start_time = start_time;
        comp.end_time = end_time;
        comp.is_active = true;
        comp.store_program = store_program;
        comp.leaderboard = Vec::with_capacity(MAX_LEADERBOARD_LEN.into());
        comp.volume_threshold = volume_threshold;
        comp.time_extension = time_extension;
        comp.max_extension = max_extension;
        comp.extension_trigger = None;
        Ok(())
    }
}
