use crate::states::{Participant, PARTICIPANT_SEED};
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
    /// CHECK: Only the address is required.
    pub competition: UncheckedAccount<'info>,

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

impl CreateParticipantIdempotent<'_> {
    /// Invoke the instruction logic.
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let p = &mut ctx.accounts.participant;
        if p.owner == Pubkey::default() {
            // First‑time initialisation.
            p.competition = ctx.accounts.competition.key();
            p.owner = ctx.accounts.trader.key();
            p.volume = 0;
            p.last_updated_at = Clock::get()?.unix_timestamp;
        }
        Ok(())
    }
}
