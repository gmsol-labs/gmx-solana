use crate::states::{Competition, Participant, PARTICIPANT_SEED};
use crate::CompetitionError;
use anchor_lang::prelude::*;

/// Create [`Participant`] account idempotently.
///
/// This instruction can be called by the store‑program (via CPI) before the
/// first trade of a trader is recorded.  
/// If the account already exists the call is a no‑op.
#[derive(Accounts)]
pub struct CreateParticipantIdempotent<'info> {
    /// Payer that funds the new PDA when it does **not** exist.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The competition account this participant belongs to.
    pub competition: Account<'info, Competition>,
    /// The participant PDA.
    #[account(
        init_if_needed,
        payer  = payer,
        space  = 8 + Participant::INIT_SPACE,
        seeds  = [
            PARTICIPANT_SEED,
            competition.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump
    )]
    pub participant: Account<'info, Participant>,
    /// The trader address.
    /// CHECK: Only the address is required.
    pub trader: UncheckedAccount<'info>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Close the [`Participant`] account.
///
/// This instruction can be called by the trader to close their participant account
/// and recover the rent.
#[derive(Accounts)]
pub struct CloseParticipant<'info> {
    /// The trader that owns the participant account.
    #[account(mut)]
    pub trader: Signer<'info>,
    /// The competition account this participant belongs to.
    pub competition: Account<'info, Competition>,
    /// The participant PDA to close.
    #[account(
        mut,
        seeds = [
            PARTICIPANT_SEED,
            competition.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump = participant.bump,
        has_one = competition,
        has_one = trader,
        close = trader,
    )]
    pub participant: Account<'info, Participant>,
}

impl CloseParticipant<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let comp = &ctx.accounts.competition;

        require!(
            now < comp.start_time || now > comp.end_time,
            CompetitionError::CompetitionInProgress
        );

        Ok(())
    }
}

impl CreateParticipantIdempotent<'_> {
    /// Invoke the instruction logic.
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        ctx.accounts
            .create_participant_idempotent(ctx.bumps.participant)
    }

    fn create_participant_idempotent(&mut self, bump: u8) -> Result<()> {
        let p = &mut self.participant;
        if p.trader == Pubkey::default() {
            let now = Clock::get()?.unix_timestamp;
            let trader = self.trader.key();
            require_keys_neq!(trader, Pubkey::default());

            p.bump = bump;
            p.competition = self.competition.key();
            p.trader = trader;
            p.volume = 0;
            p.last_updated_at = now;
        }
        Ok(())
    }
}
