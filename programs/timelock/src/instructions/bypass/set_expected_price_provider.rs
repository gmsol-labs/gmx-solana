use anchor_lang::prelude::*;
use gmsol_store::{
    cpi::{accounts::SetExpectedProvider, set_expected_provider},
    program::GmsolStore,
    states::{PriceProviderKind, RoleKey, Seed, MAX_ROLE_NAME_LEN},
    utils::{fixed_str::fixed_str_to_bytes, CpiAuthentication, WithStore},
    CoreError,
};

use crate::states::{Executor, ExecutorWalletSigner};

/// The accounts definition for [`set_expected_price_provider`](crate::gmsol_timelock::set_expected_price_provider).
#[derive(Accounts)]
pub struct SetExpectedPriceProvider<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub store: UncheckedAccount<'info>,
    /// Token map.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub token_map: UncheckedAccount<'info>,
    /// Executor.
    #[account(
        has_one = store,
        constraint = executor.load()?.role_name()? == RoleKey::MARKET_KEEPER @ CoreError::InvalidArgument,
        seeds = [
            Executor::SEED,
            store.key.as_ref(),
            &fixed_str_to_bytes::<MAX_ROLE_NAME_LEN>(RoleKey::MARKET_KEEPER)?,
        ],
        bump = executor.load()?.bump,
    )]
    pub executor: AccountLoader<'info, Executor>,
    /// Executor Wallet.
    #[account(
        mut,
        seeds = [Executor::WALLET_SEED, executor.key().as_ref()],
        bump,
    )]
    pub wallet: SystemAccount<'info>,
    /// Token to update.
    /// CHECK: only used as an identifier.
    pub token: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Revoke a role. This instruction will bypass the timelock check.
/// # CHECK
/// Only [`TIMELOCKED_MARKET_KEEPER`](crate::roles::TIMELOCKED_MARKET_KEEPER) can use.
pub(crate) fn unchecked_set_expected_price_provider(
    ctx: Context<SetExpectedPriceProvider>,
    new_expected_price_provider: PriceProviderKind,
) -> Result<()> {
    let token = ctx.accounts.token.key;
    let signer = ExecutorWalletSigner::new(ctx.accounts.executor.key(), ctx.bumps.wallet);
    let ctx = ctx.accounts.set_expected_provider_ctx();

    set_expected_provider(
        ctx.with_signer(&[&signer.as_seeds()]),
        *token,
        new_expected_price_provider.into(),
    )?;

    Ok(())
}

impl<'info> WithStore<'info> for SetExpectedPriceProvider<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for SetExpectedPriceProvider<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> SetExpectedPriceProvider<'info> {
    fn set_expected_provider_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, SetExpectedProvider<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            SetExpectedProvider {
                authority: self.wallet.to_account_info(),
                store: self.store.to_account_info(),
                token_map: self.token_map.to_account_info(),
            },
        )
    }
}
