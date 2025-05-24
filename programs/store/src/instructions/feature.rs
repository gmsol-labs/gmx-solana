use anchor_lang::prelude::*;

use crate::{
    states::{
        feature::{ActionDisabledFlag, DomainDisabledFlag},
        Store,
    },
    utils::internal,
};

/// The accounts definition for [`toggle_feature`](crate::gmsol_store::toggle_feature).
#[derive(Accounts)]
pub struct ToggleFeature<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

/// Enable or disable the given feature.
/// CHECK: only `FEATURE_KEEPER` can use this instruction.
pub(crate) fn unchecked_toggle_feature(
    ctx: Context<ToggleFeature>,
    domain: DomainDisabledFlag,
    action: ActionDisabledFlag,
    enable: bool,
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .set_feature_disabled(domain, action, !enable);
    Ok(())
}

impl<'info> internal::Authentication<'info> for ToggleFeature<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
