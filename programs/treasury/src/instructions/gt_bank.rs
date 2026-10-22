use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::Token,
    token_2022::{transfer_checked, Token2022, TransferChecked},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    cpi::{
        accounts::{CloseGtExchange, CloseGtExchangeV2 as StoreCloseGtExchangeV2},
        close_gt_exchange, close_gt_exchange_v2,
    },
    program::GmsolStore,
    states::{
        gt::{GtExchange, GtExchangeVault},
        Seed, Store,
    },
    utils::{token::validate_associated_token_account, CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::states::{Config, GtBank, TreasuryVaultConfig};

/// The accounts definition for [`prepare_gt_bank`](crate::gmsol_treasury::prepare_gt_bank).
#[derive(Accounts)]
pub struct PrepareGtBank<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        has_one = store,
        // Only allow creating GT bank for the authorized treausry.
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Config.
    #[account(
        has_one = config,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
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
            treasury_vault_config.key().as_ref(),
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
    let treasury_vault_config = ctx.accounts.treasury_vault_config.key();
    let gt_exchange_vault = ctx.accounts.gt_exchange_vault.key();

    match ctx.accounts.gt_bank.load_init() {
        Ok(mut gt_bank) => {
            gt_bank.try_init(bump, treasury_vault_config, gt_exchange_vault)?;
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
        require_keys_eq!(
            gt_bank.treasury_vault_config,
            treasury_vault_config,
            CoreError::InvalidArgument
        );
        require_keys_eq!(
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

/// Confirm a GT Bank in validator tests after Store confirmation, without an external price feed.
#[cfg(feature = "test-only")]
#[derive(Accounts)]
pub struct ConfirmGtBankForTest<'info> {
    pub authority: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    #[account(has_one = config)]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    #[account(
        has_one = store,
        constraint = gt_exchange_vault.load()?.is_confirmed() @ CoreError::PreconditionsAreNotMet,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    #[account(mut, has_one = treasury_vault_config, has_one = gt_exchange_vault)]
    pub gt_bank: AccountLoader<'info, GtBank>,
    pub store_program: Program<'info, GmsolStore>,
}

#[cfg(feature = "test-only")]
impl<'info> WithStore<'info> for ConfirmGtBankForTest<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

#[cfg(feature = "test-only")]
impl<'info> CpiAuthentication<'info> for ConfirmGtBankForTest<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

#[cfg(feature = "test-only")]
pub(crate) fn confirm_gt_bank_for_test(ctx: Context<ConfirmGtBankForTest>) -> Result<()> {
    let amount = ctx.accounts.gt_exchange_vault.load()?.amount();
    ctx.accounts.gt_bank.load_mut()?.confirm_unchecked(amount)
}

/// The accounts definition for [`sync_gt_bank_v2`](crate::gmsol_treasury::sync_gt_bank_v2).
#[derive(Accounts)]
pub struct SyncGtBank<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        has_one = store,
        // Only allow depositing into the authorized treausry.
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Vault Config.
    #[account(
        has_one = config,
        // As long as the treasury allows withdrawals of the given token,
        // deposits from GT Bank remain permitted. Disabling deposits for
        // that token won't affect GT Bank deposits already made, which
        // in turn helps the treasury recover those funds.
        constraint = treasury_vault_config.load()?.is_withdrawal_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// GT bank.
    #[account(
        mut,
        has_one = treasury_vault_config,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Treasury vault.
    #[account(
        mut,
        associated_token::authority = treasury_vault_config,
        associated_token::mint =  token,
    )]
    pub treasury_vault: InterfaceAccount<'info, TokenAccount>,
    /// GT bank vault.
    #[account(
        mut,
        associated_token::authority = gt_bank,
        associated_token::mint = token,
    )]
    pub gt_bank_vault: InterfaceAccount<'info, TokenAccount>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The token program.
    pub token_program: Interface<'info, TokenInterface>,
    /// Associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl SyncGtBank<'_> {
    /// Sync the GT bank and deposit the exceeding amount into treasury vault.
    /// # CHECK
    /// Only [`TREASURY_WITHDRAWER`](crate::roles::TREASURY_WITHDRAWER) can use.
    pub(crate) fn invoke_unchecked(ctx: Context<Self>) -> Result<u64> {
        let delta = {
            let gt_bank = ctx.accounts.gt_bank.load_mut()?;
            let token = ctx.accounts.token.key();

            let recorded_balance = gt_bank.get_balance(&token).unwrap_or(0);
            let balance = ctx.accounts.gt_bank_vault.amount;

            balance
                .checked_sub(recorded_balance)
                .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?
        };

        // Check if the balance is already in sync.
        require_neq!(delta, 0, CoreError::PreconditionsAreNotMet);

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

        ctx.accounts.gt_bank.load_mut()?.handle_synced_unchecked()?;

        Ok(delta)
    }
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
                authority: self.gt_bank.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`complete_gt_exchange`](crate::gmsol_treasury::complete_gt_exchange).
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N token mint accounts, where N represents the total number of tokens defined
///     in the GT bank.
///   - N..2N. `[mutable]` N GT bank vault accounts.
///   - 2N..3N. `[mutable]` N token accounts to receive the funds, owned by the `owner`.
#[derive(Accounts)]
pub struct CompleteGtExchange<'info> {
    /// Owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    #[account(constraint = store.load()?.validate_not_restarted().map(|_| true)?)]
    pub store: AccountLoader<'info, Store>,
    /// Config.
    #[account(
        has_one = store,
        // Only allow completing GT exchange with the authorized treasury.
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Config.
    #[account(
        has_one = config,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// GT exchange vault.
    /// CHECK: check by CPI.
    #[account(mut)]
    pub gt_exchange_vault: UncheckedAccount<'info>,
    /// GT bank.
    #[account(
        mut,
        has_one = treasury_vault_config,
        has_one = gt_exchange_vault,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    /// Exchange to complete.
    /// The ownership should be checked by the CPI.
    #[account(mut)]
    pub exchange: AccountLoader<'info, GtExchange>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The token-2022 program.
    pub token_2022_program: Program<'info, Token2022>,
}

pub(crate) fn complete_gt_exchange<'info>(
    ctx: Context<'_, '_, 'info, 'info, CompleteGtExchange<'info>>,
) -> Result<()> {
    let remaining_accounts = ctx.remaining_accounts;
    ctx.accounts.execute(remaining_accounts)?;
    Ok(())
}

impl<'info> CompleteGtExchange<'info> {
    fn execute(&self, remaining_accounts: &'info [AccountInfo<'info>]) -> Result<()> {
        use gmsol_model::num::MulDiv;

        let signer = self.config.load()?.signer();

        let gt_amount = self.exchange.load()?.amount();

        // Close GT exchange first to validate the preconditions.
        // This should validate that the GT exchange vault has been confirmed.
        let ctx = self.close_gt_exchange_ctx();
        close_gt_exchange(ctx.with_signer(&[&signer.as_seeds()]))?;

        if gt_amount == 0 {
            return Ok(());
        }

        let len = self.gt_bank.load()?.num_tokens();
        let gt_bank_tokens = self.gt_bank.load()?.tokens().collect::<Vec<_>>();
        let total_len = len.checked_mul(3).expect("must not overflow");
        require_gte!(remaining_accounts.len(), total_len);
        let tokens = &remaining_accounts[0..len];
        let vaults = &remaining_accounts[len..(2 * len)];
        let targets = &remaining_accounts[(2 * len)..total_len];

        // Transfer funds.
        {
            let gt_bank_address = self.gt_bank.key();
            let owner_address = self.owner.key();

            let gt_bank_signer = self.gt_bank.load()?.signer();
            let total_gt_amount = self.gt_bank.load()?.remaining_confirmed_gt_amount();

            require_gte!(total_gt_amount, gt_amount, CoreError::Internal);

            for (idx, token) in gt_bank_tokens.iter().enumerate() {
                let balance = self.gt_bank.load()?.get_balance(token).expect("must exist");
                if balance == 0 {
                    continue;
                }
                let amount = balance
                    .checked_mul_div(&gt_amount, &total_gt_amount)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

                let mint = &tokens[idx];
                require_keys_eq!(*mint.key, *token, CoreError::InvalidArgument);
                let token_program = if mint.owner == self.token_program.key {
                    self.token_program.to_account_info()
                } else if mint.owner == self.token_2022_program.key {
                    self.token_2022_program.to_account_info()
                } else {
                    return err!(CoreError::InvalidArgument);
                };

                let vault = &vaults[idx];
                validate_associated_token_account(
                    vault,
                    &gt_bank_address,
                    token,
                    &token_program.key(),
                )?;

                let target = &targets[idx];
                require_keys_eq!(
                    anchor_spl::token::accessor::authority(target)?,
                    owner_address
                );

                let mint = InterfaceAccount::<Mint>::try_from(mint)?;
                let decimals = mint.decimals;

                let ctx = CpiContext::new(
                    token_program,
                    TransferChecked {
                        from: vault.to_account_info(),
                        mint: mint.to_account_info(),
                        to: target.to_account_info(),
                        authority: self.gt_bank.to_account_info(),
                    },
                );

                transfer_checked(
                    ctx.with_signer(&[&gt_bank_signer.as_seeds()]),
                    amount,
                    decimals,
                )?;

                self.gt_bank
                    .load_mut()?
                    .record_transferred_out(token, amount)?;
            }

            self.gt_bank.load_mut()?.record_claimed(gt_amount)?;
        }

        Ok(())
    }

    fn close_gt_exchange_ctx(&self) -> CpiContext<'_, '_, '_, 'info, CloseGtExchange<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            CloseGtExchange {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                owner: self.owner.to_account_info(),
                vault: self.gt_exchange_vault.to_account_info(),
                exchange: self.exchange.to_account_info(),
            },
        )
    }
}

/// Complete a confirmed GT exchange without requiring its owner to sign.
///
/// Remaining accounts are N mints, N GT Bank vaults, and N owner associated
/// token accounts, in the GT Bank's token order.
#[derive(Accounts)]
pub struct CompleteGtExchangeV2<'info> {
    /// CHECK: Store close checks the exchange owner; Treasury validates its token accounts.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: Store close checks the exchange's effective rent receiver.
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
    #[account(constraint = store.load()?.validate_not_restarted().map(|_| true)?)]
    pub store: AccountLoader<'info, Store>,
    #[account(
        has_one = store,
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    #[account(has_one = config)]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    #[account(
        has_one = store,
        constraint = gt_exchange_vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.is_confirmed() @ CoreError::PreconditionsAreNotMet,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    #[account(
        mut,
        has_one = treasury_vault_config,
        has_one = gt_exchange_vault,
        constraint = gt_bank.load()?.is_confirmed() @ CoreError::PreconditionsAreNotMet,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    #[account(mut)]
    pub exchange: AccountLoader<'info, GtExchange>,
    pub store_program: Program<'info, GmsolStore>,
    pub token_program: Program<'info, Token>,
    pub token_2022_program: Program<'info, Token2022>,
}

impl<'info> CompleteGtExchangeV2<'info> {
    pub(crate) fn invoke(ctx: Context<'_, '_, 'info, 'info, Self>) -> Result<()> {
        ctx.accounts.execute(ctx.remaining_accounts)
    }

    fn execute(&self, remaining_accounts: &'info [AccountInfo<'info>]) -> Result<()> {
        use gmsol_model::num::MulDiv;

        let exchange = self.exchange.load()?;
        let gt_amount = exchange.amount();
        require_keys_eq!(
            *exchange.owner(),
            self.owner.key(),
            CoreError::InvalidArgument
        );
        require_keys_eq!(
            *exchange.rent_receiver(),
            self.rent_receiver.key(),
            CoreError::InvalidArgument
        );
        drop(exchange);

        let bank = self.gt_bank.load()?;
        let bank_tokens = bank.tokens().collect::<Vec<_>>();
        let remaining_gt = bank.remaining_confirmed_gt_amount();
        require_gte!(remaining_gt, gt_amount, CoreError::InvalidArgument);
        let expected_len = bank_tokens
            .len()
            .checked_mul(3)
            .ok_or_else(|| error!(CoreError::Internal))?;
        require_eq!(
            remaining_accounts.len(),
            expected_len,
            CoreError::InvalidArgument
        );

        let len = bank_tokens.len();
        let mints = &remaining_accounts[..len];
        let vaults = &remaining_accounts[len..2 * len];
        let targets = &remaining_accounts[2 * len..];
        let mut payouts = Vec::with_capacity(len);

        for (index, token) in bank_tokens.iter().enumerate() {
            let mint = &mints[index];
            require_keys_eq!(*mint.key, *token, CoreError::InvalidArgument);
            let token_program = if mint.owner == self.token_program.key {
                self.token_program.to_account_info()
            } else if mint.owner == self.token_2022_program.key {
                self.token_2022_program.to_account_info()
            } else {
                return err!(CoreError::InvalidArgument);
            };
            let mint_account = InterfaceAccount::<Mint>::try_from(mint)?;
            validate_associated_token_account(
                &targets[index],
                &self.owner.key(),
                token,
                &token_program.key(),
            )?;

            let balance = bank
                .get_balance(token)
                .ok_or_else(|| error!(CoreError::Internal))?;
            let amount = if gt_amount == 0 {
                0
            } else {
                balance
                    .checked_mul_div(&gt_amount, &remaining_gt)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?
            };
            if amount > 0 {
                validate_associated_token_account(
                    &vaults[index],
                    &self.gt_bank.key(),
                    token,
                    &token_program.key(),
                )?;
            }
            payouts.push((amount, mint_account.decimals, token_program));
        }
        drop(bank);

        let config_signer = self.config.load()?.signer();
        close_gt_exchange_v2(
            self.close_gt_exchange_ctx()
                .with_signer(&[&config_signer.as_seeds()]),
        )?;

        let bank_signer = self.gt_bank.load()?.signer();
        for (index, token) in bank_tokens.iter().enumerate() {
            let (amount, decimals, token_program) = &payouts[index];
            if *amount == 0 {
                continue;
            }
            transfer_checked(
                CpiContext::new(
                    token_program.clone(),
                    TransferChecked {
                        from: vaults[index].clone(),
                        mint: mints[index].clone(),
                        to: targets[index].clone(),
                        authority: self.gt_bank.to_account_info(),
                    },
                )
                .with_signer(&[&bank_signer.as_seeds()]),
                *amount,
                *decimals,
            )?;
            self.gt_bank
                .load_mut()?
                .record_transferred_out(token, *amount)?;
        }
        self.gt_bank.load_mut()?.record_claimed(gt_amount)?;
        Ok(())
    }

    fn close_gt_exchange_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, StoreCloseGtExchangeV2<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            StoreCloseGtExchangeV2 {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                owner: self.owner.to_account_info(),
                vault: self.gt_exchange_vault.to_account_info(),
                rent_receiver: self.rent_receiver.to_account_info(),
                exchange: self.exchange.to_account_info(),
            },
        )
    }
}
