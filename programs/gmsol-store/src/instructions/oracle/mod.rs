/// Instructions for custom price feed.
pub mod custom;

use std::ops::Deref;

use anchor_lang::prelude::*;

use crate::{
    states::{Chainlink, Oracle, PriceValidator, Store, TokenMapHeader, TokenMapLoader},
    utils::internal,
};

pub use self::custom::*;

/// The accounts definition for [`initialize_oracle`](crate::gmsol_store::initialize_oracle).
///
/// [*See also the documentation for the instruction.*](crate::gmsol_store::initialize_oracle)
#[derive(Accounts)]
pub struct InitializeOracle<'info> {
    /// The payer, and it will be set as the authority of the created oracle account.
    pub payer: Signer<'info>,
    /// The store account that will be the owner of the oracle account.
    pub store: AccountLoader<'info, Store>,
    /// The new oracle account.
    #[account(zero)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn initialize_oracle(ctx: Context<InitializeOracle>) -> Result<()> {
    ctx.accounts
        .oracle
        .load_init()?
        .init(ctx.accounts.store.key(), ctx.accounts.payer.key());
    Ok(())
}

/// The accounts definition for [`clear_all_prices`](crate::gmsol_store::clear_all_prices).
#[derive(Accounts)]
pub struct ClearAllPrices<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Oracle.
    #[account(
        mut,
        has_one = store,
        has_one = authority,
    )]
    pub oracle: AccountLoader<'info, Oracle>,
}

/// Clear all prices of the given oracle account.
/// CHECK: only ORACLE_CONTROLLER is allowed to invoke.
pub(crate) fn unchecked_clear_all_prices(ctx: Context<ClearAllPrices>) -> Result<()> {
    ctx.accounts.oracle.load_mut()?.clear_all_prices();
    Ok(())
}

impl<'info> internal::Authentication<'info> for ClearAllPrices<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`set_prices_from_price_feed`](crate::gmsol_store::set_prices_from_price_feed).
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N feed accounts, where N represents the total number of tokens.
#[derive(Accounts)]
pub struct SetPricesFromPriceFeed<'info> {
    /// The caller.
    pub authority: Signer<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Oracle.
    #[account(
        mut,
        has_one = store,
        has_one = authority,
    )]
    pub oracle: AccountLoader<'info, Oracle>,
    /// Token map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Chainlink Program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// Set the oracle prices from price feeds.
/// CHECK: only ORACLE_CONTROLLER is allowed to invoke.
pub(crate) fn unchecked_set_prices_from_price_feed<'info>(
    ctx: Context<'_, '_, 'info, 'info, SetPricesFromPriceFeed<'info>>,
    tokens: Vec<Pubkey>,
) -> Result<()> {
    let validator = PriceValidator::try_from(ctx.accounts.store.load()?.deref())?;
    let token_map = ctx.accounts.token_map.load_token_map()?;
    ctx.accounts
        .oracle
        .load_mut()?
        .set_prices_from_remaining_accounts(
            validator,
            &token_map,
            &tokens,
            ctx.remaining_accounts,
            ctx.accounts.chainlink_program.as_ref(),
        )
}

impl<'info> internal::Authentication<'info> for SetPricesFromPriceFeed<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
