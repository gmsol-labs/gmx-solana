use crate::states::{Amount, Factor};
use anchor_lang::prelude::*;

use crate::{states::Store, utils::internal};

/// The accounts definition of instructions for updating configs.
#[derive(Accounts)]
pub struct InsertConfig<'info> {
    /// Caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

impl<'info> internal::Authentication<'info> for InsertConfig<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// CHECK: only CONFIG_KEEPER is allowed to invoke.
pub(crate) fn unchecked_insert_amount(
    ctx: Context<InsertConfig>,
    key: &str,
    amount: Amount,
) -> Result<()> {
    *ctx.accounts.store.load_mut()?.get_amount_mut(key)? = amount;
    Ok(())
}

/// CHECK: only CONFIG_KEEPER is allowed to invoke, except for:
///
/// - [`OrderFeeDiscountForReferredUser`](crate::states::FactorKey::OrderFeeDiscountForReferredUser)
///   which can also be updated using
///   [`insert_order_fee_discount_for_referred_user`](crate::gmsol_store::insert_order_fee_discount_for_referred_user)
pub(crate) fn unchecked_insert_factor(
    ctx: Context<InsertConfig>,
    key: &str,
    factor: Factor,
) -> Result<()> {
    *ctx.accounts.store.load_mut()?.get_factor_mut(key)? = factor;
    Ok(())
}

/// CHECK: only CONFIG_KEEPER is allowed to invoke.
pub(crate) fn unchecked_insert_address(
    ctx: Context<InsertConfig>,
    key: &str,
    address: Pubkey,
) -> Result<()> {
    *ctx.accounts.store.load_mut()?.get_address_mut(key)? = address;
    Ok(())
}
