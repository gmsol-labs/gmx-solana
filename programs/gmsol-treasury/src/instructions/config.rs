use anchor_lang::prelude::*;
use gmsol_store::{
    program::GmsolStore,
    states::{Seed, Store},
    utils::{CpiAuthentication, WithStore},
};
use gmsol_utils::InitSpace;

use crate::states::{config::Config, treasury::TreasuryConfig};

/// The accounts definition for [`initialize_config`](crate::gmsol_treasury::initialize_config).
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Payer.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The store that controls this config.
    pub store: AccountLoader<'info, Store>,
    /// The config account.
    #[account(
        init,
        payer = payer,
        space = 8 + Config::INIT_SPACE,
        seeds = [Config::SEED, store.key().as_ref()],
        bump,
    )]
    pub config: AccountLoader<'info, Config>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn initialize_config(ctx: Context<InitializeConfig>) -> Result<()> {
    let mut config = ctx.accounts.config.load_init()?;
    let store = ctx.accounts.store.key();
    config.init(ctx.bumps.config, &store);
    msg!("[Treasury] initialized the treasury config for {}", store);
    Ok(())
}

/// The accounts definition for [`set_treasury`](crate::gmsol_treasury::set_treasury).
#[derive(Accounts)]
pub struct SetTreasury<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to update.
    #[account(mut, has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Treasury.
    #[account(has_one = config)]
    pub treasury: AccountLoader<'info, TreasuryConfig>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Set config's treasury address.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_set_treasury(ctx: Context<SetTreasury>) -> Result<()> {
    let treasury = ctx.accounts.treasury.key();
    let previous = ctx
        .accounts
        .config
        .load_mut()?
        .set_treasury_config(treasury)?;
    msg!(
        "[Treasury] the treasury address has been updated from {} to {}",
        previous,
        treasury
    );
    Ok(())
}

impl<'info> WithStore<'info> for SetTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for SetTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(gmsol_store::CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`set_gt_factor`](crate::gmsol_treasury::set_gt_factor).
#[derive(Accounts)]
pub struct SetGtFactor<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to update.
    #[account(mut, has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Set config's gt factor.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_set_gt_factor(ctx: Context<SetGtFactor>, factor: u128) -> Result<()> {
    let previous = ctx.accounts.config.load_mut()?.set_gt_factor(factor)?;
    msg!(
        "[Treasury] the GT factor has been updated from {} to {}",
        previous,
        factor
    );
    Ok(())
}

impl<'info> WithStore<'info> for SetGtFactor<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for SetGtFactor<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(gmsol_store::CoreError::PermissionDenied)
    }
}
