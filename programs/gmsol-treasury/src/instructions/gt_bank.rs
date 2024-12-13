use anchor_lang::prelude::*;
use gmsol_store::{
    program::GmsolStore,
    states::{gt::GtExchangeVault, Seed},
    utils::{CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::{Config, GtBank};

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
            config.key().as_ref(),
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
    let config = ctx.accounts.config.key();
    let gt_exchange_vault = ctx.accounts.gt_exchange_vault.key();

    match ctx.accounts.gt_bank.load_init() {
        Ok(mut gt_bank) => {
            gt_bank.try_init(bump, gt_exchange_vault, config)?;
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
        require_eq!(gt_bank.config, config, CoreError::InvalidArgument);
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
