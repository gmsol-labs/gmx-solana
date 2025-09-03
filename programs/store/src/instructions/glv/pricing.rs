use anchor_lang::prelude::*;
use anchor_spl::{token::Mint, token_interface};
use gmsol_model::utils::market_token_amount_to_usd;
use gmsol_utils::swap::SwapActionParams;

use crate::{
    events::{EventEmitter, GlvTokenValue},
    ops::glv::get_glv_value_for_market_with_new_index_price,
    states::{Glv, Market, MaxAgeValidator, Oracle, Store, TokenMapHeader, TokenMapLoader},
    CoreError,
};

/// The accounts definition for [`get_glv_token_value`](crate::gmsol_store::get_glv_token_value).
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N market accounts, where N represents the total number of markets managed
///     by the given GLV.
///   - N..2N. `[]` N market token accounts (see above for the definition of N).
///   - 2N..2N+M. `[]` M feed accounts, where M represents the total number of tokens associated with
///     markets in the given GLV, sorted by token address.
#[event_cpi]
#[derive(Accounts)]
pub struct GetGlvTokenValue<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token Map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Oracle buffer to use.
    #[account(mut, has_one = store, has_one = authority)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// GLV account.
    #[account(has_one = store)]
    pub glv: AccountLoader<'info, Glv>,
    /// GLV token mint.
    #[account(constraint = glv.load()?.glv_token == glv_token.key() @ CoreError::TokenMintMismatched)]
    pub glv_token: Box<InterfaceAccount<'info, token_interface::Mint>>,
}

impl<'info> GetGlvTokenValue<'info> {
    pub(crate) fn invoke(
        ctx: Context<'_, '_, 'info, 'info, Self>,
        amount: u64,
        maximize: bool,
        max_age: u32,
        emit_event: bool,
    ) -> Result<u128> {
        let accounts = ctx.accounts;

        accounts.evaluate(
            amount,
            maximize,
            max_age,
            emit_event.then_some(ctx.bumps.event_authority),
            ctx.remaining_accounts,
        )
    }

    fn evaluate(
        &self,
        amount: u64,
        maximize: bool,
        max_age: u32,
        emit_event: Option<u8>,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<u128> {
        let splitted = {
            let token_map = self.token_map.load_token_map()?;
            self.glv.load()?.validate_and_split_remaining_accounts(
                &self.store.key(),
                remaining_accounts,
                None::<&SwapActionParams>,
                &token_map,
            )?
        };

        let glv = self.glv.load()?;
        self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &splitted.tokens,
            splitted.remaining_accounts,
            |oracle, _| {
                oracle.validate_time(&MaxAgeValidator::new(max_age))?;
                let mut prices = None;
                let glv_value: u128 = splitted
                    .markets
                    .iter()
                    .zip(splitted.market_tokens)
                    .map(|(market, market_token)| {
                        let key = market_token.key();
                        let balance = u128::from(
                            glv.market_config(&key)
                                .ok_or_else(|| error!(CoreError::NotFound))?
                                .balance(),
                        );
                        let market = AccountLoader::<Market>::try_from(market)?;
                        let mint = Account::<Mint>::try_from(market_token)?;
                        let market = market.load()?;
                        let market = market.as_liquidity_market(&mint);
                        let prices = match prices.as_mut() {
                            Some(prices) => prices,
                            None => {
                                let oracle_prices = oracle.market_prices(&market)?;
                                prices.get_or_insert(oracle_prices)
                            }
                        };
                        let market_token_value = get_glv_value_for_market_with_new_index_price(
                            oracle, prices, &market, balance, maximize,
                        )?
                        .market_token_value_in_glv;
                        Result::<_>::Ok(market_token_value)
                    })
                    .sum::<Result<_>>()?;
                let supply = self.glv_token.supply;
                let value = market_token_amount_to_usd(
                    &u128::from(amount),
                    &glv_value,
                    &u128::from(supply),
                )
                .ok_or_else(|| error!(CoreError::FailedToCalculateGlvValueForMarket))?;

                if let Some(bump) = emit_event {
                    let event_emitter = EventEmitter::new(&self.event_authority, bump);
                    event_emitter.emit_cpi(&GlvTokenValue {
                        glv_token: self.glv_token.key(),
                        supply,
                        is_value_maximized: maximize,
                        glv_value,
                        amount,
                        value,
                    })?;
                }

                Ok(value)
            },
        )
    }
}
