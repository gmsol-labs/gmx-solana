use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{transfer_checked, TransferChecked},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    program::GmsolStore,
    states::{gt::GtExchangeVault, Seed, Store},
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::{config::Config, treasury::TreasuryConfig, GtBank};

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
    /// Treasury config account to initialize.
    #[account(
        init,
        payer = authority,
        space = 8 + TreasuryConfig::INIT_SPACE,
        seeds = [TreasuryConfig::SEED, config.key().as_ref(), &[index]],
        bump,
    )]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
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
    ctx.accounts.treasury_config.load_init()?.init(
        ctx.bumps.treasury_config,
        index,
        &ctx.accounts.config.key(),
    );
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
    /// Treasury config.
    #[account(mut, has_one = config)]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
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
    ctx.accounts
        .treasury_config
        .load_mut()?
        .insert_token(&token)?;
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
    /// Treasury Config.
    #[account(mut, has_one = config)]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
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
    ctx.accounts
        .treasury_config
        .load_mut()?
        .remove_token(token)?;
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
    /// Treasury Config.
    #[account(mut, has_one = config)]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
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
    let previous = ctx.accounts.treasury_config.load_mut()?.toggle_token_flag(
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
    pub store: AccountLoader<'info, Store>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Config.
    #[account(
        has_one = config,
        constraint = treasury_config.load()?.is_deposit_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
    /// GT exchange vault.
    #[account(
        has_one = store,
        constraint = store.load()?.gt().exchange_time_window() as i64 == gt_exchange_vault.load()?.time_window() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.validate_depositable().map(|_| true)?,
        seeds = [GtExchangeVault::SEED, store.key().as_ref(), &gt_exchange_vault.load()?.time_window_index().to_be_bytes()],
        bump = gt_exchange_vault.load()?.bump,
        seeds::program = gmsol_store::ID,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    /// GT bank.
    #[account(
        mut,
        has_one = config,
        has_one = gt_exchange_vault,
        seeds = [
            GtBank::SEED,
            config.key().as_ref(),
            gt_exchange_vault.key().as_ref(),
        ],
        bump = gt_bank.load()?.bump,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
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
        associated_token::authority = treasury_config,
        associated_token::mint =  token,
    )]
    pub treasury_vault: InterfaceAccount<'info, TokenAccount>,
    /// GT bank vault.
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::authority = gt_bank,
        associated_token::mint =  token,
    )]
    pub gt_bank_vault: InterfaceAccount<'info, TokenAccount>,
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
    use gmsol_model::utils::apply_factor;
    use gmsol_store::constants::{MARKET_DECIMALS, MARKET_USD_UNIT};

    let signer = ctx.accounts.config.load()?.signer();
    let decimals = ctx.accounts.token.decimals;

    let (gt_amount, treasury_amount): (u64, u64) = {
        let gt_factor = ctx.accounts.config.load()?.gt_factor();
        let amount = u128::from(ctx.accounts.receiver_vault.amount);
        require_gte!(MARKET_USD_UNIT, gt_factor, CoreError::Internal);
        let gt_amount = apply_factor::<_, MARKET_DECIMALS>(&amount, &gt_factor)
            .ok_or_else(|| error!(CoreError::Internal))?;
        let treasury_amount = amount
            .checked_sub(gt_amount)
            .ok_or_else(|| error!(CoreError::Internal))?;
        (
            gt_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
            treasury_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        )
    };

    let cpi_ctx = ctx.accounts.transfer_checked_ctx_for_gt_bank();
    transfer_checked(
        cpi_ctx.with_signer(&[&signer.as_seeds()]),
        gt_amount,
        decimals,
    )?;

    ctx.accounts
        .gt_bank
        .load_mut()?
        .record_transferred_in(&ctx.accounts.token.key(), gt_amount)?;

    let cpi_ctx = ctx.accounts.transfer_checked_ctx_for_treasury();
    transfer_checked(
        cpi_ctx.with_signer(&[&signer.as_seeds()]),
        treasury_amount,
        decimals,
    )?;
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
    fn transfer_checked_ctx_for_treasury(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
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

    fn transfer_checked_ctx_for_gt_bank(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.receiver_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.gt_bank_vault.to_account_info(),
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
    /// Treasury Config.
    #[account(
        has_one = config,
        constraint = treasury_config.load()?.is_withdrawal_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Treasury vault.
    #[account(
        mut,
        associated_token::authority = treasury_config,
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
    let signer = ctx.accounts.treasury_config.load()?.signer();
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
