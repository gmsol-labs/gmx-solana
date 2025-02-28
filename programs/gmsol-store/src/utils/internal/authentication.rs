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
            self.store().load()?.has_admin_role(self.authority().key)?,
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

    /// Check that the `authority` has the [`ORACLE_CONTROLLER`](`RoleKey::ORACLE_CONTROLLER`) role.
    fn only_oracle_controller(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::ORACLE_CONTROLLER)
    }

    /// Check that the `authority` has the [`GT_CONTROLLER`](`RoleKey::GT_CONTROLLER`) role.
    fn only_gt_controller(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::GT_CONTROLLER)
    }

    /// Check that the `authority` has the [`MARKET_KEEPER`](`RoleKey::MARKET_KEEPER`) role.
    fn only_market_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::MARKET_KEEPER)
    }

    /// Check that the `authority` has the [`ORDER_KEEPER`](`RoleKey::ORDER_KEEPER`) role.
    fn only_order_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::ORDER_KEEPER)
    }

    /// Check that the `authority` has the [`FEATURE_KEEPER`](`RoleKey::FEATURE_KEEPER`) role.
    fn only_feature_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::FEATURE_KEEPER)
    }

    /// Check that the `authority` has the [`CONFIG_KEEPER`](`RoleKey::CONFIG_KEEPER`) role.
    fn only_config_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::CONFIG_KEEPER)
    }

    /// Check that the `authority` has the [`PRICE_KEEPER`](`RoleKey::PRICE_KEEPER`) role.
    fn only_price_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::PRICE_KEEPER)
    }

    /// Check that the `authority` has the [`MIGRATION_KEEPER`](`RoleKey::MIGRATION_KEEPER`) role.
    fn only_migration_keeper(ctx: &Context<Self>) -> Result<()> {
        Self::only(ctx, RoleKey::MIGRATION_KEEPER)
    }
}

impl<'info, T> Authenticate<'info> for T where T: Authentication<'info> + Bumps + Sized {}
