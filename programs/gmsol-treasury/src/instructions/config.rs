use anchor_lang::prelude::*;
use gmsol_store::states::Seed;
use gmsol_utils::InitSpace;

use crate::states::config::Config;

/// The accounts definition for [`initialize`](crate::gmsol_treasury::initialize_config).
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The store that controls this config.
    /// CHECK: only need to check that it is owned by the store program.
    #[account(owner = gmsol_store::ID)]
    pub store: UncheckedAccount<'info>,
    /// The config account.
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [Config::SEED, store.key.as_ref()],
        bump,
    )]
    pub config: AccountLoader<'info, Config>,
    system_program: Program<'info, System>,
}

pub(crate) fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
    let mut config = ctx.accounts.config.load_init()?;
    let store = ctx.accounts.store.key;
    config.init(ctx.bumps.config, store);
    msg!("[Treasury] initialized the treasury config for {}", store);
    Ok(())
}
