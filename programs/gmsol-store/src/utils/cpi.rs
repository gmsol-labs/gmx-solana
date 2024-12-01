use anchor_lang::{prelude::*, Bumps};

use crate::{
    cpi::accounts::{CheckRole, ClearAllPrices, SetPricesFromPriceFeed},
    states::RoleKey,
};

/// With Store.
pub trait WithStore<'info> {
    /// Get data store program.
    fn store_program(&self) -> AccountInfo<'info>;

    /// Get data store.
    fn store(&self) -> AccountInfo<'info>;
}

/// Accounts that can be used for authentication.
pub trait CpiAuthentication<'info>: Bumps + Sized + WithStore<'info> {
    /// Get the authority to check.
    ///
    /// ## Notes
    /// - `authority` should be a signer.
    fn authority(&self) -> AccountInfo<'info>;

    /// Get the cpi context for checking role or admin permission.
    fn check_role_ctx(&self) -> CpiContext<'_, '_, '_, 'info, CheckRole<'info>> {
        CpiContext::new(
            self.store_program(),
            CheckRole {
                authority: self.authority(),
                store: self.store(),
            },
        )
    }

    /// Callback on authentication error.
    fn on_error(&self) -> Result<()>;
}

/// Provides access control utils for [`CpiAuthentication`]s.
pub trait CpiAuthenticate<'info>: CpiAuthentication<'info> {
    /// Check that the `authority` has the given `role`.
    fn only(ctx: &Context<Self>, role: &str) -> Result<()> {
        let has_role =
            crate::cpi::check_role(ctx.accounts.check_role_ctx(), role.to_string())?.get();
        if has_role {
            Ok(())
        } else {
            ctx.accounts.on_error()
        }
    }

    /// Check that the `authority` is an admin.
    fn only_admin(ctx: &Context<Self>) -> Result<()> {
        let is_admin = crate::cpi::check_admin(ctx.accounts.check_role_ctx())?.get();
        if is_admin {
            Ok(())
        } else {
            ctx.accounts.on_error()
        }
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

impl<'info, T> CpiAuthenticate<'info> for T where T: CpiAuthentication<'info> {}

/// Accounts that with oracle context.
pub trait WithOracle<'info>: WithStore<'info> {
    /// Get the chainlink program.
    fn chainlink_program(&self) -> Option<AccountInfo<'info>>;

    /// Get the oracle account.
    fn oracle(&self) -> AccountInfo<'info>;

    /// Get the token map account.
    fn token_map(&self) -> AccountInfo<'info>;

    /// Get controller account.
    fn controller(&self) -> AccountInfo<'info>;
}

/// Extension trait for [`WithOracle`].
pub trait WithOracleExt<'info>: WithOracle<'info> {
    /// Get the CPI context for set prices.
    fn set_prices_from_price_feed_ctx(
        &self,
        feeds: Vec<AccountInfo<'info>>,
    ) -> CpiContext<'_, '_, '_, 'info, SetPricesFromPriceFeed<'info>> {
        CpiContext::new(
            self.store_program().to_account_info(),
            SetPricesFromPriceFeed {
                authority: self.controller(),
                store: self.store(),
                token_map: self.token_map(),
                oracle: self.oracle(),
                chainlink_program: self.chainlink_program(),
            },
        )
        .with_remaining_accounts(feeds)
    }

    /// Get the CPI context for clear all prices.
    fn clear_all_prices_ctx(&self) -> CpiContext<'_, '_, '_, 'info, ClearAllPrices<'info>> {
        CpiContext::new(
            self.store_program().to_account_info(),
            ClearAllPrices {
                authority: self.controller(),
                store: self.store(),
                oracle: self.oracle(),
            },
        )
    }

    /// Run the given function inside the scope with oracle prices.
    fn with_oracle_prices<T>(
        &mut self,
        tokens: Vec<Pubkey>,
        remaining_accounts: &'info [AccountInfo<'info>],
        signer_seeds: &[&[u8]],
        f: impl FnOnce(&mut Self, &'info [AccountInfo<'info>]) -> Result<T>,
    ) -> Result<T> {
        require_gte!(
            remaining_accounts.len(),
            tokens.len(),
            ErrorCode::AccountNotEnoughKeys
        );
        let feeds = remaining_accounts[..tokens.len()].to_vec();
        let remaining_accounts = &remaining_accounts[tokens.len()..];
        crate::cpi::set_prices_from_price_feed(
            self.set_prices_from_price_feed_ctx(feeds)
                .with_signer(&[signer_seeds]),
            tokens,
        )?;
        let output = f(self, remaining_accounts)?;
        crate::cpi::clear_all_prices(self.clear_all_prices_ctx().with_signer(&[signer_seeds]))?;
        Ok(output)
    }
}

impl<'info, T> WithOracleExt<'info> for T where T: WithOracle<'info> {}
