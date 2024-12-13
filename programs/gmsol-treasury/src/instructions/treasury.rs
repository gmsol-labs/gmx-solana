use anchor_lang::prelude::*;
use anchor_spl::token_interface::TokenAccount;
use gmsol_store::{
    program::GmsolStore,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};

use crate::states::{config::Config, treasury::Treasury};

/// The accounts definition for [`initialize_treasury`](crate::gmsol_treasury::initialize_treasury).
#[derive(Accounts)]
pub struct InitializeTreasury<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Treasury account to initialize.
    #[account(zero)]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Initialize [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_OWNER`](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_initialize_treasury(ctx: Context<InitializeTreasury>) -> Result<()> {
    ctx.accounts
        .treasury
        .load_init()?
        .init(&ctx.accounts.config.key());
    Ok(())
}

impl<'info> WithStore<'info> for InitializeTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InitializeTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`insert_token_to_treasury`](crate::gmsol_treasury::insert_token_to_treasury).
#[derive(Accounts)]
pub struct InsertTokenToTreasury<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    #[account(mut, has_one = config)]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Token to insert.
    pub token: InterfaceAccount<'info, TokenAccount>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Insert a token to the [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_OWNER`](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_insert_token_to_treasury(
    ctx: Context<InsertTokenToTreasury>,
) -> Result<()> {
    ctx.accounts
        .treasury
        .load_mut()?
        .insert_token(&ctx.accounts.token.key())?;
    Ok(())
}

impl<'info> WithStore<'info> for InsertTokenToTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InsertTokenToTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`toggle_token_flag`](crate::gmsol_treasury::toggle_token_flag).
#[derive(Accounts)]
pub struct ToggleTokenFlag<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    #[account(mut, has_one = config)]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Token.
    pub token: InterfaceAccount<'info, TokenAccount>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Toggle a token flag.
/// # CHECK
/// Only [`TREASURY_OWNER`](crate::roles::TREASURY_OWNER) can use.
pub(crate) fn unchecked_toggle_token_flag(
    ctx: Context<ToggleTokenFlag>,
    flag: &str,
    value: bool,
) -> Result<()> {
    let previous = ctx.accounts.treasury.load_mut()?.toggle_token_flag(
        &ctx.accounts.token.key(),
        flag.parse()
            .map_err(|_| error!(CoreError::InvalidArgument))?,
        value,
    )?;
    msg!(
        "[Treasury] toggled token config flag {}: {} -> {}",
        flag,
        previous,
        value
    );
    Ok(())
}

impl<'info> WithStore<'info> for ToggleTokenFlag<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ToggleTokenFlag<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}
