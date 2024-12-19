use anchor_lang::prelude::*;
use gmsol_store::{
    program::GmsolStore,
    states::Seed,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::config::TimelockConfig;

/// The accounts definition for [`initialize_config`](crate::gmsol_timelock::initialize_config).
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        init,
        payer = authority,
        space = 8 + TimelockConfig::INIT_SPACE,
        seeds = [TimelockConfig::SEED, store.key.as_ref()],
        bump,
    )]
    pub timelock_config: AccountLoader<'info, TimelockConfig>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Initialize timelock config.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_initialize_config(
    ctx: Context<InitializeConfig>,
    delay: u32,
) -> Result<()> {
    ctx.accounts.timelock_config.load_init()?.init(
        ctx.bumps.timelock_config,
        delay,
        ctx.accounts.store.key(),
    );
    msg!(
        "[Timelock] Initialized timelock config with delay = {}",
        delay
    );
    Ok(())
}

impl<'info> WithStore<'info> for InitializeConfig<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InitializeConfig<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`increase_delay`](crate::gmsol_timelock::increase_delay).
#[derive(Accounts)]
pub struct IncreaseDelay<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    #[account(mut, has_one = store)]
    pub timelock_config: AccountLoader<'info, TimelockConfig>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Increase delay.
/// # CHECK
/// Only [`TIMELOCK_ADMIN`](crate::roles::TIMELOCK_ADMIN) can use.
pub(crate) fn unchecked_increase_delay(ctx: Context<IncreaseDelay>, delta: u32) -> Result<()> {
    require_neq!(delta, 0, CoreError::InvalidArgument);
    let new_delay = ctx
        .accounts
        .timelock_config
        .load_mut()?
        .increase_delay(delta)?;
    msg!(
        "[Timelock] Timelock delay increased, new delay = {}",
        new_delay
    );
    Ok(())
}

impl<'info> WithStore<'info> for IncreaseDelay<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for IncreaseDelay<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}
