use anchor_lang::prelude::*;
use gmsol_store::{
    cpi::{accounts::RevokeRole as StoreRevokeRole, revoke_role},
    program::GmsolStore,
    states::{Seed, MAX_ROLE_NAME_LEN},
    utils::{fixed_str::fixed_str_to_bytes, CpiAuthentication, WithStore},
    CoreError,
};

use crate::{roles, states::Executor};

/// The accounts definition for [`revoke_role`](crate::gmsol_timelock::revoke_role).
#[derive(Accounts)]
pub struct RevokeRole<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub store: UncheckedAccount<'info>,
    /// Executor.
    #[account(
        has_one = store,
        constraint = executor.load()?.role_name()? == roles::ADMIN @ CoreError::InvalidArgument,
        seeds = [
            Executor::SEED,
            store.key.as_ref(),
            &fixed_str_to_bytes::<MAX_ROLE_NAME_LEN>(roles::ADMIN)?,
        ],
        bump = executor.load()?.bump,
    )]
    pub executor: AccountLoader<'info, Executor>,
    /// User.
    /// CHECK: only its address is used.
    pub user: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Revoke a role. This instruction will bypass the timelock check.
/// # CHECK
/// Only [`TIMELOCKED_ADMIN`](roles::TIMELOCKED_ADMIN) can use.
pub(crate) fn unchecked_revoke_role(ctx: Context<RevokeRole>, role: String) -> Result<()> {
    let signer = ctx.accounts.executor.load()?.signer();
    let cpi_ctx = ctx.accounts.revoke_role_ctx();
    revoke_role(
        cpi_ctx.with_signer(&[&signer.as_seeds()]),
        ctx.accounts.user.key(),
        role,
    )?;
    Ok(())
}

impl<'info> WithStore<'info> for RevokeRole<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for RevokeRole<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> RevokeRole<'info> {
    fn revoke_role_ctx(&self) -> CpiContext<'_, '_, '_, 'info, StoreRevokeRole<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            StoreRevokeRole {
                authority: self.executor.to_account_info(),
                store: self.store.to_account_info(),
            },
        )
    }
}
