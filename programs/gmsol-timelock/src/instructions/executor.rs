use anchor_lang::prelude::*;
use gmsol_store::{
    states::{Seed, MAX_ROLE_NAME_LEN},
    utils::fixed_str::fixed_str_to_bytes,
};
use gmsol_utils::InitSpace;

use crate::states::executor::Executor;

/// The accounts definition for [`initialize_executor`](crate::gmsol_timelock::initialize_executor).
#[derive(Accounts)]
#[instruction(role: String)]
pub struct InitializeExecutor<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Executor to initialize.
    #[account(
        init,
        payer = payer,
        space = 8 + Executor::INIT_SPACE,
        seeds = [
            Executor::SEED,
            store.key.as_ref(),
            &fixed_str_to_bytes::<MAX_ROLE_NAME_LEN>(&role)?,
        ],
        bump,
    )]
    pub executor: AccountLoader<'info, Executor>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn initialize_executor(ctx: Context<InitializeExecutor>, role: &str) -> Result<()> {
    ctx.accounts.executor.load_init()?.try_init(
        ctx.bumps.executor,
        ctx.accounts.store.key(),
        role,
    )?;
    Ok(())
}
