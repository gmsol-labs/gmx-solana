use anchor_lang::prelude::*;

use crate::{
    internal,
    states::{market::utils::ClosableMarket, Market, Oracle, Store, TokenMapHeader},
};

/// The accounts definition for [`update_closed_state`](crate::gmsol_store::update_closed_state).
///
/// *[See also the documentation for the instruction.](crate::gmsol_store::update_closed_state)*
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N feed accounts, where N represents the total number of unique tokens
///     associated to the market.
#[derive(Accounts)]
pub struct UpdateClosedState<'info> {
    /// The address authorized to execute this instruction.
    pub authority: Signer<'info>,
    /// The store that owns the market.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// The oracle buffer to use.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// The market to update the ADL state.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

impl<'info> UpdateClosedState<'info> {
    /// CHECK: only ORDER_KEEPER is authorized to perform this action.
    pub(crate) fn invoke_unchecked(ctx: Context<'_, '_, 'info, 'info, Self>) -> Result<()> {
        let mut market = ctx.accounts.market.load_mut()?;
        let tokens = market
            .meta()
            .ordered_tokens()
            .into_iter()
            .collect::<Vec<_>>();

        ctx.accounts.oracle.load_mut()?.with_prices_opts(
            &ctx.accounts.store,
            &ctx.accounts.token_map,
            &tokens,
            ctx.remaining_accounts,
            |oracle, _remaining_accounts| market.update_closed_state(oracle),
            true,
        )?;
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for UpdateClosedState<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
