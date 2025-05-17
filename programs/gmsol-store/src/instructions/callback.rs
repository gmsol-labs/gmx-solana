use anchor_lang::prelude::*;
use gmsol_callback::CALLBACK_AUTHORITY_SEED;

use crate::states::callback::CallbackAuthority;

/// Initialize the [`CallbackAuthority`] account.
#[derive(Accounts)]
pub struct InitializeCallbackAuthority<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The callback authority account.
    #[account(
        init,
        payer = payer,
        space = 8 + CallbackAuthority::INIT_SPACE,
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump,
    )]
    pub callback_authority: Account<'info, CallbackAuthority>,
    /// System program.
    pub system_program: Program<'info, System>,
}

impl InitializeCallbackAuthority<'_> {
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        ctx.accounts.callback_authority.bump_bytes = [ctx.bumps.callback_authority];
        Ok(())
    }
}
