use anchor_lang::prelude::*;

use crate::{states::Store, utils::internal};

/// The accounts definition for [`check_admin`](crate::gmsol_store::check_admin)
/// and [`check_role`](crate::gmsol_store::check_role).
#[derive(Accounts)]
pub struct CheckRole<'info> {
    /// The address to check for the role.
    pub authority: Signer<'info>,
    /// The store account in which the role is defined.
    pub store: AccountLoader<'info, Store>,
}

/// Verify that the `user` is an admin of the given `store`.
pub(crate) fn check_admin(ctx: Context<CheckRole>) -> Result<bool> {
    ctx.accounts
        .store
        .load()?
        .has_admin_role(ctx.accounts.authority.key)
}

/// Verify that the `authority` has the given role in the given `store`.
pub(crate) fn check_role(ctx: Context<CheckRole>, role: String) -> Result<bool> {
    ctx.accounts
        .store
        .load()?
        .has_role(ctx.accounts.authority.key, &role)
}

/// The accounts definition for [`has_admin`](crate::gmsol_store::has_admin)
/// and [`has_role`](crate::gmsol_store::has_role).
#[derive(Accounts)]
pub struct HasRole<'info> {
    /// The store account in which the role is defined.
    pub store: AccountLoader<'info, Store>,
}

/// Verify that the `user` is an admin of the given `store` without signing.
pub fn has_admin(ctx: Context<HasRole>, authority: Pubkey) -> Result<bool> {
    ctx.accounts.store.load()?.has_admin_role(&authority)
}

/// Verify that the `authority` has the given role in the given `store` without signing.
pub fn has_role(ctx: Context<HasRole>, authority: Pubkey, role: String) -> Result<bool> {
    ctx.accounts.store.load()?.has_role(&authority, &role)
}

/// The accounts definition for [`enable_role`](crate::gmsol_store::enable_role).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::enable_role).*
#[derive(Accounts)]
pub struct EnableRole<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    /// The store account for which the role is to be added/enabled.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

/// Enable the given role in the data store.
///
/// # CHECK
/// - This instruction can only be called by the `ADMIN`.
pub(crate) fn unchecked_enable_role(ctx: Context<EnableRole>, role: String) -> Result<()> {
    ctx.accounts.store.load_mut()?.enable_role(&role)
}

impl<'info> internal::Authentication<'info> for EnableRole<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`disable_role`](crate::gmsol_store::disable_role).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::disable_role).*
#[derive(Accounts)]
pub struct DisableRole<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    /// The store account for which the role is to be disabled.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

/// Disable the given role in the data store.
///
/// # CHECK
/// - This instruction can only be called by the `ADMIN`.
pub fn unchecked_disable_role(ctx: Context<DisableRole>, role: String) -> Result<()> {
    ctx.accounts.store.load_mut()?.disable_role(&role)
}

impl<'info> internal::Authentication<'info> for DisableRole<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`grant_role`](crate::gmsol_store::grant_role).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::grant_role).*
#[derive(Accounts)]
pub struct GrantRole<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    #[account(mut)]
    /// The store account to which the new role is to be granted.
    pub store: AccountLoader<'info, Store>,
}

/// Grant a role to the user.
///
/// # CHECK
/// - This instruction can only be called by the `ADMIN`.
pub(crate) fn unchecked_grant_role(
    ctx: Context<GrantRole>,
    user: Pubkey,
    role: String,
) -> Result<()> {
    ctx.accounts.store.load_mut()?.grant(&user, &role)
}

impl<'info> internal::Authentication<'info> for GrantRole<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`revoke_role`](crate::gmsol_store::revoke_role).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::revoke_role).*
#[derive(Accounts)]
pub struct RevokeRole<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    /// The store account from which the new role is to be revoked.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

/// Revoke a role to the user.
///
/// # CHECK
/// - This instruction can only be called by the `ADMIN`.
pub(crate) fn unchecked_revoke_role(
    ctx: Context<RevokeRole>,
    user: Pubkey,
    role: String,
) -> Result<()> {
    ctx.accounts.store.load_mut()?.revoke(&user, &role)
}

impl<'info> internal::Authentication<'info> for RevokeRole<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
