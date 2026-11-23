use anchor_lang::{prelude::*, system_program};

use crate::{constants::RENT_COLLECTOR_SEED, CoreError};

/// Accounts for [`claim_rent`](crate::gmsol_store::claim_rent).
#[derive(Accounts)]
pub struct ClaimRent<'info> {
    /// The receiver authorizing withdrawal from its collector.
    pub rent_receiver: Signer<'info>,
    /// The receiver's system-owned collector. It needs no initialization or data.
    #[account(
        mut,
        seeds = [RENT_COLLECTOR_SEED, rent_receiver.key().as_ref()],
        bump,
    )]
    pub rent_collector: SystemAccount<'info>,
    /// CHECK: the signing receiver chooses this account to receive its lamports.
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,
    /// System program, which owns and transfers the collector's lamports.
    pub system_program: Program<'info, System>,
}

impl ClaimRent<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let amount = ctx.accounts.rent_collector.lamports();
        if amount == 0 {
            return Ok(());
        }
        // A successful nonempty claim must move the funds out of the collector.
        require_keys_neq!(
            ctx.accounts.rent_collector.key(),
            ctx.accounts.destination.key(),
            CoreError::InvalidArgument
        );

        let receiver = ctx.accounts.rent_receiver.key();
        let seeds = &[
            RENT_COLLECTOR_SEED,
            receiver.as_ref(),
            &[ctx.bumps.rent_collector],
        ];
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.rent_collector.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                },
            )
            .with_signer(&[seeds]),
            amount,
        )
    }
}
