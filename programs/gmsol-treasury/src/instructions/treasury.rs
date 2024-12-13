use anchor_lang::prelude::*;
use gmsol_store::{
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};

use crate::states::{config::Config, treasury::Treasury};

/// The accounts definition for [`initialize_treasury`](crate::gmsol_treasury::initialize_treasury).
#[derive(Accounts)]
pub struct InitializeTreasury<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Treasury account to initialize.
    #[account(zero)]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Initialize [`Treasury`] account.
/// # CHECK
/// Only [TREASURY_OWNER](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_initialize_treasury(ctx: Context<InitializeTreasury>) -> Result<()> {
    ctx.accounts
        .treasury
        .load_init()?
        .init(&ctx.accounts.config.key());
    Ok(())
}

impl<'info> WithStore<'info> for InitializeTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InitializeTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}
