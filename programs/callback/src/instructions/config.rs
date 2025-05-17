use anchor_lang::prelude::*;

use crate::states::{Config, CONFIG_SEED, MAX_PREFIX_LEN};

/// Initialize the [`Config`] account.
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The [`Config`] account to initialize.
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

impl InitializeConfig<'_> {
    pub(crate) fn invoke(ctx: Context<Self>, prefix: String) -> Result<()> {
        require_gte!(usize::from(MAX_PREFIX_LEN), prefix.len());
        ctx.accounts.config.prefix = prefix;
        ctx.accounts.config.calls = 0;
        Ok(())
    }
}
