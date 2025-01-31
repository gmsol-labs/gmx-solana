use anchor_lang::prelude::*;
use gmsol_store::{
    program::GmsolStore,
    states::{Seed, Store, MAX_ROLE_NAME_LEN},
    utils::{fixed_str::fixed_str_to_bytes, CpiAuthenticate, CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::{
    roles,
    states::{config::TimelockConfig, Executor, ExecutorWalletSigner},
};

/// The accounts definition for [`initialize_config`](crate::gmsol_timelock::initialize_config).
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// Config.
    #[account(
        init,
        payer = authority,
        space = 8 + TimelockConfig::INIT_SPACE,
        seeds = [TimelockConfig::SEED, store.key().as_ref()],
        bump,
    )]
    pub timelock_config: AccountLoader<'info, TimelockConfig>,
    /// Admin executor.
    #[account(
        has_one = store,
        constraint = executor.load()?.role_name()? == roles::ADMIN @ CoreError::InvalidArgument,
        seeds = [
            Executor::SEED,
            store.key().as_ref(),
            &fixed_str_to_bytes::<MAX_ROLE_NAME_LEN>(roles::ADMIN)?,
        ],
        bump = executor.load()?.bump,
    )]
    pub executor: AccountLoader<'info, Executor>,
    /// Admin executor wallet.
    #[account(
        seeds = [Executor::WALLET_SEED, executor.key().as_ref()],
        bump,
    )]
    pub wallet: SystemAccount<'info>,
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
    // Ensure at least one address can create and execute timelocked instructions.
    CpiAuthenticate::only(&ctx, roles::TIMELOCK_KEEPER)?;
    // Ensure at least one address can approve timelocked instructions requiring ADMIN permission,
    // such as granting roles to other addresses.
    CpiAuthenticate::only(&ctx, roles::TIMELOCKED_ADMIN)?;

    let admin_executor_wallet_bump = ctx.bumps.wallet;

    ctx.accounts
        .accept_store_authority(admin_executor_wallet_bump)?;

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

impl InitializeConfig<'_> {
    fn accept_store_authority(&self, admin_executor_wallet_bump: u8) -> Result<()> {
        use gmsol_store::cpi::{accept_store_authority, accounts::AcceptStoreAuthority};

        if !self.store.load()?.is_authority(self.wallet.key) {
            let signer = ExecutorWalletSigner::new(self.executor.key(), admin_executor_wallet_bump);
            accept_store_authority(
                CpiContext::new(
                    self.store_program.to_account_info(),
                    AcceptStoreAuthority {
                        next_authority: self.wallet.to_account_info(),
                        store: self.store.to_account_info(),
                    },
                )
                .with_signer(&[&signer.as_seeds()]),
            )?;
        }

        Ok(())
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
