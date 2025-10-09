use anchor_lang::prelude::*;

use crate::{
    events::EventEmitter,
    internal,
    ops::market::RemainingAccountsForMarket,
    states::{
        market::{
            revertible::{Revertible, RevertibleMarket},
            utils::ClosableMarket,
        },
        Market, MarketPriceOptions, Oracle, Store, TokenMapHeader,
    },
};

/// The accounts definitions for the [`update_fees_state`](crate::gmsol_store::update_fees_state).
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N feed accounts, where N represents the total number of unique tokens
///     in the market.
///   - N..N+V. `[writable]` V virtual inventory accounts, where V represents the total
///     number of unique virtual inventories required by the markets.
#[event_cpi]
#[derive(Accounts)]
pub struct UpdateFeesState<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut, has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Buffer for oracle prices.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
}

impl<'info> UpdateFeesState<'info> {
    /// CHECK: only ORDER_KEEPER is allowed to use this instruction.
    pub(crate) fn invoke_unchecked(ctx: Context<'_, '_, 'info, 'info, Self>) -> Result<()> {
        ctx.accounts.validate()?;
        ctx.accounts
            .update(ctx.remaining_accounts, ctx.bumps.event_authority)?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        self.market
            .load()?
            .validate_with_options(&self.store.key(), true)?;
        Ok(())
    }

    fn update(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_authority_bump: u8,
    ) -> Result<()> {
        let tokens = self
            .market
            .load()?
            .meta()
            .ordered_tokens()
            .into_iter()
            .collect::<Vec<_>>();

        let event_emitter = EventEmitter::new(&self.event_authority, event_authority_bump);
        let current_market_token = self.market.load()?.meta().market_token_mint;

        self.oracle.load_mut()?.with_prices_opts(
            &self.store,
            &self.token_map,
            &tokens,
            remaining_accounts,
            |oracle, remaining_accounts| {
                // Validate market closed state.
                {
                    let store = self.store.load()?;
                    self.market
                        .load()?
                        .validate_open_or_nonstale_oracle_with_store(oracle, &store)?;
                }
                let remaining_accounts = RemainingAccountsForMarket::new(
                    remaining_accounts,
                    current_market_token,
                    None,
                )?;
                let virtual_inventories = remaining_accounts.load_virtual_inventories()?;
                let mut market =
                    RevertibleMarket::new(&self.market, Some(&virtual_inventories), event_emitter)?;
                let prices = oracle.market_prices_with_options(
                    &market,
                    MarketPriceOptions {
                        allow_index_closed: true,
                        allow_long_closed: false,
                        allow_short_closed: false,
                    },
                )?;
                market.update_fees_state(&prices)?;

                market.commit();
                virtual_inventories.commit();
                Ok(())
            },
            true,
        )?;
        Ok(())
    }
}

impl<'info> internal::Authentication<'info> for UpdateFeesState<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
