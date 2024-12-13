use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{transfer_checked, TransferChecked},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    program::GmsolStore,
    states::Seed,
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::{config::Config, treasury::Treasury};

/// The accounts definition for [`initialize_treasury`](crate::gmsol_treasury::initialize_treasury).
#[derive(Accounts)]
#[instruction(index: u8)]
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
    #[account(
        init,
        payer = authority,
        space = 8 + Treasury::INIT_SPACE,
        seeds = [Treasury::SEED, config.key().as_ref(), &[index]],
        bump,
    )]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Initialize [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_initialize_treasury(
    ctx: Context<InitializeTreasury>,
    index: u8,
) -> Result<()> {
    ctx.accounts
        .treasury
        .load_init()?
        .init(ctx.bumps.treasury, index, &ctx.accounts.config.key());
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
    pub token: InterfaceAccount<'info, Mint>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Insert a token to the [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_insert_token_to_treasury(
    ctx: Context<InsertTokenToTreasury>,
) -> Result<()> {
    let token = ctx.accounts.token.key();
    ctx.accounts.treasury.load_mut()?.insert_token(&token)?;
    msg!(
        "[Treasury] inserted a token into the treasury, token = {}",
        token
    );
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

/// The accounts definition for [`remove_token_from_treasury`](crate::gmsol_treasury::remove_token_from_treasury).
#[derive(Accounts)]
pub struct RemoveTokenFromTreasury<'info> {
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
    /// Token to remove.
    /// CHECK: only used as a identifier.
    pub token: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Remove a token from the [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_remove_token_from_treasury(
    ctx: Context<RemoveTokenFromTreasury>,
) -> Result<()> {
    let token = ctx.accounts.token.key;
    ctx.accounts.treasury.load_mut()?.remove_token(token)?;
    msg!(
        "[Treasury] removed a token from the treasury, token = {}",
        token
    );
    Ok(())
}

impl<'info> WithStore<'info> for RemoveTokenFromTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for RemoveTokenFromTreasury<'info> {
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
    pub token: InterfaceAccount<'info, Mint>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Toggle a token flag.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
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

/// The accounts definition for [`deposit_into_treasury`](crate::gmsol_treasury::deposit_into_treasury).
#[derive(Accounts)]
pub struct DepositIntoTreasury<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    #[account(
        has_one = config,
        constraint = treasury.load()?.is_deposit_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Receiver vault.
    #[account(
        mut,
        associated_token::authority = config,
        associated_token::mint =  token,
    )]
    pub receiver_vault: InterfaceAccount<'info, TokenAccount>,
    /// Treasury vault.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::authority = treasury,
        associated_token::mint =  token,
    )]
    pub treasury_vault: InterfaceAccount<'info, TokenAccount>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The token program.
    pub token_program: Interface<'info, TokenInterface>,
    /// Associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Deposit tokens from the receiver vault to the treasury vault.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_deposit_into_treasury(ctx: Context<DepositIntoTreasury>) -> Result<()> {
    let signer = ctx.accounts.config.load()?.signer();
    let cpi_ctx = ctx.accounts.transfer_checked_ctx();
    let amount = ctx.accounts.receiver_vault.amount;
    let decimals = ctx.accounts.token.decimals;
    transfer_checked(cpi_ctx.with_signer(&[&signer.as_seeds()]), amount, decimals)?;
    Ok(())
}

impl<'info> WithStore<'info> for DepositIntoTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for DepositIntoTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> DepositIntoTreasury<'info> {
    fn transfer_checked_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.receiver_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.treasury_vault.to_account_info(),
                authority: self.config.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`withdraw_from_treasury`](crate::gmsol_treasury::withdraw_from_treasury).
#[derive(Accounts)]
pub struct WithdrawFromTreasury<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    #[account(
        has_one = config,
        constraint = treasury.load()?.is_withdrawal_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury: AccountLoader<'info, Treasury>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Treasury vault.
    #[account(
        mut,
        associated_token::authority = treasury,
        associated_token::mint =  token,
    )]
    pub treasury_vault: InterfaceAccount<'info, TokenAccount>,
    /// Target.
    #[account(mut, token::mint = token)]
    pub target: InterfaceAccount<'info, TokenAccount>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The token program.
    pub token_program: Interface<'info, TokenInterface>,
}

/// Withdraw tokens from the treasury vault.
/// # CHECK
/// Only [`TREASURY_WITHDRAWER`](crate::roles::TREASURY_WITHDRAWER) can use.
pub(crate) fn unchecked_withdraw_from_treasury(
    ctx: Context<WithdrawFromTreasury>,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let signer = ctx.accounts.treasury.load()?.signer();
    let cpi_ctx = ctx.accounts.transfer_checked_ctx();
    transfer_checked(cpi_ctx.with_signer(&[&signer.as_seeds()]), amount, decimals)?;
    Ok(())
}

impl<'info> WithStore<'info> for WithdrawFromTreasury<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for WithdrawFromTreasury<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> WithdrawFromTreasury<'info> {
    fn transfer_checked_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.treasury_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.target.to_account_info(),
                authority: self.config.to_account_info(),
            },
        )
    }
}
