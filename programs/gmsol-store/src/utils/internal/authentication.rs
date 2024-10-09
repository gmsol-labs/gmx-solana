use anchor_lang::{prelude::*, Bumps};

use crate::{
    states::{RoleKey, Store},
    CoreError,
};

/// Accounts that can be used for authentication.
pub(crate) trait Authentication<'info> {
    /// Get the authority to check.
    fn authority(&self) -> &Signer<'info>;

    /// Get the data store account.
    fn store(&self) -> &AccountLoader<'info, Store>;

    /// Check that the `authority` is an admin.
    fn only_admin(&self) -> Result<()> {
        require!(
            self.store().load()?.is_authority(self.authority().key),
            CoreError::NotAnAdmin
        );
        Ok(())
    }

    /// Check that the `authority` has the given `role`.
    fn only_role(&self, role: &str) -> Result<()> {
        require!(
            self.store().load()?.has_role(self.authority().key, role)?,
            CoreError::PermissionDenied
        );
        Ok(())
    }
}

/// Provides access control utils for [`Authentication`]s.
pub(crate) trait Authenticate<'info>: Authentication<'info> + Bumps + Sized {
    /// Check that the `authority` has the given `role`.
    fn only(ctx: &Context<Self>, role: &str) -> Result<()> {
        ctx.accounts.only_role(role)
    }

    /// Check that the `authority` is an admin.
    fn only_admin(ctx: &Context<Self>) -> Result<()> {
        ctx.accounts.only_admin()
    }

    /// Check that the `authority` has the [`CONTROLLER`](`RoleKey::CONTROLLER`) role.
    fn only_controller(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::CONTROLLER)
    }

    /// Check that the `authority` has the [`MARKET_KEEPER`](`RoleKey::MARKET_KEEPER`) role.
    fn only_market_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::MARKET_KEEPER)
    }

    /// Check that the `authority` has the [`ORDER_KEEPER`](`RoleKey::ORDER_KEEPER`) role.
    fn only_order_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::ORDER_KEEPER)
    }
}

impl<'info, T> Authenticate<'info> for T where T: Authentication<'info> + Bumps + Sized {}
