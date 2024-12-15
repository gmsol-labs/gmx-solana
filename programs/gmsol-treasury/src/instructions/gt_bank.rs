use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{transfer_checked, TransferChecked},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    program::GmsolStore,
    states::{gt::GtExchangeVault, Seed},
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::{Config, GtBank, TreasuryConfig};

/// The accounts definition for [`prepare_gt_bank`](crate::gmsol_treasury::prepare_gt_bank).
#[derive(Accounts)]
pub struct PrepareGtBank<'info> {
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
    )]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
    /// GT exchange vault.
    #[account(
        has_one = store,
        constraint = gt_exchange_vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = !gt_exchange_vault.load()?.is_confirmed() @ CoreError::InvalidArgument,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    /// GT Bank.
    #[account(
        init_if_needed,
        payer = authority,
        space = 8 + GtBank::INIT_SPACE,
        seeds = [
            GtBank::SEED,
            treasury_config.key().as_ref(),
            gt_exchange_vault.key().as_ref(),
        ],
        bump,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Prepare a GT Bank.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_prepare_gt_bank(ctx: Context<PrepareGtBank>) -> Result<()> {
    let bump = ctx.bumps.gt_bank;
    let treasury_config = ctx.accounts.treasury_config.key();
    let gt_exchange_vault = ctx.accounts.gt_exchange_vault.key();

    match ctx.accounts.gt_bank.load_init() {
        Ok(mut gt_bank) => {
            gt_bank.try_init(bump, treasury_config, gt_exchange_vault)?;
            drop(gt_bank);
            ctx.accounts.gt_bank.exit(&crate::ID)?;
        }
        Err(Error::AnchorError(err)) => {
            if err.error_code_number != ErrorCode::AccountDiscriminatorAlreadySet as u32 {
                return Err(Error::AnchorError(err));
            }
        }
        Err(err) => {
            return Err(err);
        }
    }

    // Validate.
    {
        let gt_bank = ctx.accounts.gt_bank.load()?;
        require_eq!(gt_bank.bump, bump, CoreError::InvalidArgument);
        require_eq!(
            gt_bank.treasury_config,
            treasury_config,
            CoreError::InvalidArgument
        );
        require_eq!(
            gt_bank.gt_exchange_vault,
            gt_exchange_vault,
            CoreError::InvalidArgument
        );
        require!(gt_bank.is_initialized(), CoreError::InvalidArgument);
    }

    Ok(())
}

impl<'info> WithStore<'info> for PrepareGtBank<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for PrepareGtBank<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`sync_gt_bank`](crate::gmsol_treasury::sync_gt_bank).
#[derive(Accounts)]
pub struct SyncGtBank<'info> {
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
        constraint = treasury_config.load()?.is_deposit_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_config: AccountLoader<'info, TreasuryConfig>,
    /// GT bank.
    #[account(
        mut,
        has_one = treasury_config,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
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

/// Sync the GT bank and deposit the exceeding amount into treasury vault.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_sync_gt_bank(ctx: Context<SyncGtBank>) -> Result<()> {
    let delta = {
        let gt_bank = ctx.accounts.gt_bank.load_mut()?;
        let token = ctx.accounts.token.key();

        let recorded_balance = gt_bank.get_balance(&token).unwrap_or(0);
        let balance = ctx.accounts.gt_bank_vault.amount;

        require_gte!(balance, recorded_balance, CoreError::NotEnoughTokenAmount);

        balance
            .checked_sub(recorded_balance)
            .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?
    };

    if delta != 0 {
        let cpi_ctx = ctx.accounts.transfer_checked_ctx();
        let signer = ctx.accounts.gt_bank.load()?.signer();
        transfer_checked(
            cpi_ctx.with_signer(&[&signer.as_seeds()]),
            delta,
            ctx.accounts.token.decimals,
        )?;
        msg!(
            "[Treasury] Synced GT Bank balance, deposit exceeding {} tokens into treasury",
            delta
        );
    }

    Ok(())
}

impl<'info> WithStore<'info> for SyncGtBank<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for SyncGtBank<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> SyncGtBank<'info> {
    fn transfer_checked_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.gt_bank_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.treasury_vault.to_account_info(),
                authority: self.config.to_account_info(),
            },
        )
    }
}
