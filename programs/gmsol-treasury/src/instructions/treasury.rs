use std::collections::BTreeSet;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_2022::{transfer_checked, TransferChecked},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use gmsol_store::{
    cpi::{
        accounts::{ClearAllPrices, ConfirmGtExchangeVault, SetPricesFromPriceFeed},
        clear_all_prices, confirm_gt_exchange_vault_v2, set_prices_from_price_feed,
    },
    program::GmsolStore,
    states::{gt::GtExchangeVault, Chainlink, Oracle, Seed, Store},
    utils::{token::is_associated_token_account_with_program_id, CpiAuthentication, WithStore},
    CoreError,
};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    states::{
        config::{Config, ReceiverSigner},
        treasury::TreasuryVaultConfig,
        GtBank,
    },
};

/// The accounts definition for [`initialize_treasury_vault_config`](crate::gmsol_treasury::initialize_treasury_vault_config).
#[derive(Accounts)]
#[instruction(index: u16)]
pub struct InitializeTreasuryVaultConfig<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config to initialize with.
    #[account(has_one = store)]
    pub config: AccountLoader<'info, Config>,
    /// Treasury vault config account to initialize.
    #[account(
        init,
        payer = authority,
        space = 8 + TreasuryVaultConfig::INIT_SPACE,
        seeds = [TreasuryVaultConfig::SEED, config.key().as_ref(), &index.to_le_bytes()],
        bump,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

/// Initialize [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_initialize_treasury_vault_config(
    ctx: Context<InitializeTreasuryVaultConfig>,
    index: u16,
) -> Result<()> {
    ctx.accounts.treasury_vault_config.load_init()?.init(
        ctx.bumps.treasury_vault_config,
        index,
        &ctx.accounts.config.key(),
    );
    Ok(())
}

impl<'info> WithStore<'info> for InitializeTreasuryVaultConfig<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InitializeTreasuryVaultConfig<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`insert_token_to_treasury_vault`](crate::gmsol_treasury::insert_token_to_treasury_vault).
#[derive(Accounts)]
pub struct InsertTokenToTreasuryVault<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        has_one = store,
        // Insert to an unauthorized treasury vault config is allowed.
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury vault config.
    #[account(mut, has_one = config)]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// Token to insert.
    pub token: InterfaceAccount<'info, Mint>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Insert a token to the [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_insert_token_to_treasury_vault(
    ctx: Context<InsertTokenToTreasuryVault>,
) -> Result<()> {
    let token = ctx.accounts.token.key();
    ctx.accounts
        .treasury_vault_config
        .load_mut()?
        .insert_token(&token)?;
    msg!(
        "[Treasury] inserted a token into the treasury, token = {}",
        token
    );
    Ok(())
}

impl<'info> WithStore<'info> for InsertTokenToTreasuryVault<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for InsertTokenToTreasuryVault<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

/// The accounts definition for [`remove_token_from_treasury_vault`](crate::gmsol_treasury::remove_token_from_treasury_vault).
#[derive(Accounts)]
pub struct RemoveTokenFromTreasuryVault<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        has_one = store,
        // Remove from an unauthorized treasury vault config is allowed.
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Vault Config.
    #[account(mut, has_one = config)]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// Token to remove.
    /// CHECK: only used as a identifier.
    pub token: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
}

/// Remove a token from the [`Treasury`] account.
/// # CHECK
/// Only [`TREASURY_ADMIN`](crate::roles::TREASURY_ADMIN) can use.
pub(crate) fn unchecked_remove_token_from_treasury_vault(
    ctx: Context<RemoveTokenFromTreasuryVault>,
) -> Result<()> {
    let token = ctx.accounts.token.key;
    ctx.accounts
        .treasury_vault_config
        .load_mut()?
        .remove_token(token)?;
    msg!(
        "[Treasury] removed a token from the treasury, token = {}",
        token
    );
    Ok(())
}

impl<'info> WithStore<'info> for RemoveTokenFromTreasuryVault<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for RemoveTokenFromTreasuryVault<'info> {
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
    /// Config.
    #[account(
        has_one = store,
        // Toggle flags of an unauthorized treasury vault config is allowed.
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Vault Config.
    #[account(mut, has_one = config)]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
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
    let previous = ctx
        .accounts
        .treasury_vault_config
        .load_mut()?
        .toggle_token_flag(
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

/// The accounts definition for [`deposit_to_treasury_vault`](crate::gmsol_treasury::deposit_to_treasury_vault).
#[derive(Accounts)]
pub struct DepositToTreasuryVault<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Config.
    #[account(
        has_one = store,
        // Only allow depositing into the authorized treausry vault.
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Config.
    #[account(
        has_one = config,
        constraint = treasury_vault_config.load()?.is_deposit_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// Receiver.
    #[account(
        seeds = [constants::RECEIVER_SEED, config.key().as_ref()],
        bump,
    )]
    pub receiver: SystemAccount<'info>,
    /// GT exchange vault.
    #[account(
        has_one = store,
        constraint = store.load()?.gt().exchange_time_window() as i64 == gt_exchange_vault.load()?.time_window() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.validate_depositable().map(|_| true)?,
        seeds = [
            GtExchangeVault::SEED,
            store.key().as_ref(),
            &gt_exchange_vault.load()?.time_window_index().to_le_bytes(),
            &gt_exchange_vault.load()?.time_window_u32().to_le_bytes(),
        ],
        bump = gt_exchange_vault.load()?.bump,
        seeds::program = gmsol_store::ID,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    /// GT bank.
    #[account(
        mut,
        has_one = treasury_vault_config,
        has_one = gt_exchange_vault,
        seeds = [
            GtBank::SEED,
            treasury_vault_config.key().as_ref(),
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
        associated_token::authority = receiver,
        associated_token::mint = token,
    )]
    pub receiver_vault: InterfaceAccount<'info, TokenAccount>,
    /// Treasury vault.
    #[account(
        mut,
        associated_token::authority = treasury_vault_config,
        associated_token::mint = token,
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

/// Deposit tokens from the receiver vault to the treasury vault.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_deposit_to_treasury_vault(
    ctx: Context<DepositToTreasuryVault>,
) -> Result<()> {
    use gmsol_model::utils::apply_factor;
    use gmsol_store::constants::{MARKET_DECIMALS, MARKET_USD_UNIT};

    let signer = ReceiverSigner::new(ctx.accounts.config.key(), ctx.bumps.receiver);
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

impl<'info> WithStore<'info> for DepositToTreasuryVault<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for DepositToTreasuryVault<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> DepositToTreasuryVault<'info> {
    fn transfer_checked_ctx_for_treasury(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.receiver_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.treasury_vault.to_account_info(),
                authority: self.receiver.to_account_info(),
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
                authority: self.receiver.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`withdraw_from_treasury_vault`](crate::gmsol_treasury::withdraw_from_treasury_vault).
#[derive(Accounts)]
pub struct WithdrawFromTreasuryVault<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    /// CHECK: check by CPI.
    pub store: UncheckedAccount<'info>,
    /// Config.
    #[account(
        has_one = store,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Vault Config.
    #[account(
        has_one = config,
        constraint = treasury_vault_config.load()?.is_withdrawal_allowed(&token.key())? @ CoreError::InvalidArgument,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// Token.
    pub token: InterfaceAccount<'info, Mint>,
    /// Treasury vault.
    #[account(
        mut,
        associated_token::authority = treasury_vault_config,
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
pub(crate) fn unchecked_withdraw_from_treasury_vault(
    ctx: Context<WithdrawFromTreasuryVault>,
    amount: u64,
    decimals: u8,
) -> Result<()> {
    let signer = ctx.accounts.treasury_vault_config.load()?.signer();
    let cpi_ctx = ctx.accounts.transfer_checked_ctx();
    transfer_checked(cpi_ctx.with_signer(&[&signer.as_seeds()]), amount, decimals)?;
    Ok(())
}

impl<'info> WithStore<'info> for WithdrawFromTreasuryVault<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for WithdrawFromTreasuryVault<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> WithdrawFromTreasuryVault<'info> {
    fn transfer_checked_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: self.treasury_vault.to_account_info(),
                mint: self.token.to_account_info(),
                to: self.target.to_account_info(),
                authority: self.treasury_vault_config.to_account_info(),
            },
        )
    }
}

/// The accounts definition for [`confirm_gt_buyback`](crate::gmsol_treasury::confirm_gt_buyback).
///
/// Remaining accounts expected by this instruction:
///
///   - 0..N. `[]` N feed accounts sorted by token addresses, where N represents the total number of tokens defined in
///     the GT bank or the treasury vault config.
///   - N..(N+M). `[]` M token mint accounts, where M represents the total number of tokens defined in
///     the treasury vault config.
///   - (N+M)..(N+2M). `[]` M treasury vault token accounts.
#[derive(Accounts)]
pub struct ConfirmGtBuyback<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// Config.
    #[account(
        has_one = store,
        // Only allow confirming buyback with the authorized treausry.
        constraint = config.load()?.treasury_vault_config() == Some(&treasury_vault_config.key()) @ CoreError::InvalidArgument,
    )]
    pub config: AccountLoader<'info, Config>,
    /// Treasury Vault Config.
    #[account(
        has_one = config,
    )]
    pub treasury_vault_config: AccountLoader<'info, TreasuryVaultConfig>,
    /// GT exchange vault.
    #[account(
        mut,
        has_one = store,
        constraint = gt_exchange_vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = gt_exchange_vault.load()?.validate_confirmable().map(|_| true)? @ CoreError::InvalidArgument,
    )]
    pub gt_exchange_vault: AccountLoader<'info, GtExchangeVault>,
    /// GT Bank.
    #[account(
        mut,
        has_one = treasury_vault_config,
        has_one = gt_exchange_vault,
    )]
    pub gt_bank: AccountLoader<'info, GtBank>,
    /// Token map.
    /// CHECK: check by CPI.
    pub token_map: UncheckedAccount<'info>,
    /// Oracle.
    /// CHECK: the permissions should be checked by the CPI.
    #[account(mut)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// Event authority.
    /// CHECK: check by CPI.
    pub event_authority: UncheckedAccount<'info>,
    /// Store program.
    pub store_program: Program<'info, GmsolStore>,
    /// Chainlink program.
    pub chainlink_program: Option<Program<'info, Chainlink>>,
}

/// Confirm GT buyback.
/// # CHECK
/// Only [`TREASURY_KEEPER`](crate::roles::TREASURY_KEEPER) can use.
pub(crate) fn unchecked_confirm_gt_buyback<'info>(
    ctx: Context<'_, '_, 'info, 'info, ConfirmGtBuyback<'info>>,
) -> Result<()> {
    ctx.accounts.execute(ctx.remaining_accounts)
}

impl<'info> WithStore<'info> for ConfirmGtBuyback<'info> {
    fn store_program(&self) -> AccountInfo<'info> {
        self.store_program.to_account_info()
    }

    fn store(&self) -> AccountInfo<'info> {
        self.store.to_account_info()
    }
}

impl<'info> CpiAuthentication<'info> for ConfirmGtBuyback<'info> {
    fn authority(&self) -> AccountInfo<'info> {
        self.authority.to_account_info()
    }

    fn on_error(&self) -> Result<()> {
        err!(CoreError::PermissionDenied)
    }
}

impl<'info> ConfirmGtBuyback<'info> {
    fn validate_and_split_remaining_accounts(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        num_tokens: usize,
    ) -> Result<(&'info [AccountInfo<'info>], &'info [AccountInfo<'info>])> {
        let num_treasury_tokens = self.treasury_vault_config.load()?.num_tokens();
        let treasury_tokens_end = num_tokens
            .checked_add(num_treasury_tokens)
            .ok_or_else(|| error!(CoreError::Internal))?;
        let end = treasury_tokens_end
            .checked_add(num_treasury_tokens)
            .ok_or_else(|| error!(CoreError::Internal))?;
        require_gte!(
            remaining_accounts.len(),
            end,
            ErrorCode::AccountNotEnoughKeys
        );

        let feeds = &remaining_accounts[0..num_tokens];
        let mints = &remaining_accounts[num_tokens..treasury_tokens_end];
        let vaults = &remaining_accounts[treasury_tokens_end..end];

        let treasury_vault_config_key = self.treasury_vault_config.key();
        for (idx, token) in self.treasury_vault_config.load()?.tokens().enumerate() {
            let mint = &mints[idx];
            require_keys_eq!(mint.key(), token, CoreError::TokenMintMismatched);
            let vault = &vaults[idx];
            require_keys_eq!(*mint.owner, *vault.owner, CoreError::InvalidArgument);

            let token_program_id = mint.owner;

            let mint = InterfaceAccount::<Mint>::try_from(mint)?;
            let vault = InterfaceAccount::<TokenAccount>::try_from(vault)?;

            require_keys_eq!(vault.mint, mint.key(), CoreError::TokenMintMismatched);
            require_keys_eq!(
                vault.owner,
                treasury_vault_config_key,
                CoreError::InvalidArgument
            );
            require!(
                is_associated_token_account_with_program_id(
                    &vault.key(),
                    &treasury_vault_config_key,
                    &token,
                    token_program_id
                ),
                CoreError::InvalidArgument
            );
        }

        Ok((feeds, vaults))
    }

    fn execute(&mut self, remaining_accounts: &'info [AccountInfo<'info>]) -> Result<()> {
        let signer = self.config.load()?.signer();

        let total_gt_amount = self.gt_exchange_vault.load()?.amount();
        self.gt_bank.load_mut()?.unchecked_confirm(total_gt_amount);

        let tokens = self
            .gt_bank
            .load()?
            .tokens()
            .chain(self.treasury_vault_config.load()?.tokens())
            .collect::<BTreeSet<_>>();

        let (feeds, vaults) =
            self.validate_and_split_remaining_accounts(remaining_accounts, tokens.len())?;

        // Set prices.
        let ctx = self.set_prices_from_price_feed_ctx();
        set_prices_from_price_feed(
            ctx.with_signer(&[&signer.as_seeds()])
                .with_remaining_accounts(feeds.to_vec()),
            tokens.iter().copied().collect(),
        )?;

        let (buyback_value, buyback_price) = self.update_balances(vaults, total_gt_amount)?;

        // Clear prices.
        let ctx = self.clear_all_prices_ctx();
        clear_all_prices(ctx.with_signer(&[&signer.as_seeds()]))?;

        let ctx = self.confirm_gt_exchange_vault_ctx();
        confirm_gt_exchange_vault_v2(
            ctx.with_signer(&[&signer.as_seeds()]),
            buyback_value,
            buyback_price,
        )?;

        Ok(())
    }

    fn set_prices_from_price_feed_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, SetPricesFromPriceFeed<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            SetPricesFromPriceFeed {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                oracle: self.oracle.to_account_info(),
                token_map: self.token_map.to_account_info(),
                chainlink_program: self.chainlink_program.as_ref().map(|a| a.to_account_info()),
            },
        )
    }

    fn clear_all_prices_ctx(&self) -> CpiContext<'_, '_, '_, 'info, ClearAllPrices<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            ClearAllPrices {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                oracle: self.oracle.to_account_info(),
            },
        )
    }

    fn confirm_gt_exchange_vault_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, ConfirmGtExchangeVault<'info>> {
        CpiContext::new(
            self.store_program.to_account_info(),
            ConfirmGtExchangeVault {
                authority: self.config.to_account_info(),
                store: self.store.to_account_info(),
                vault: self.gt_exchange_vault.to_account_info(),
                event_authority: self.event_authority.to_account_info(),
                program: self.store_program.to_account_info(),
            },
        )
    }

    fn get_max_buyback_value(&self, vaults: &[AccountInfo<'info>]) -> Result<BuybackValue> {
        use anchor_spl::token::accessor;
        use gmsol_model::utils::apply_factor;

        let oracle = self.oracle.load()?;
        let gt_bank_value = self.gt_bank.load()?.total_value(&oracle)?;

        let mut treasury_value = 0u128;
        for vault in vaults {
            let token = accessor::mint(vault)?;
            let amount = accessor::amount(vault)?;
            let price = oracle.get_primary_price(&token, false)?.min;
            let value = u128::from(amount)
                .checked_mul(price)
                .ok_or_else(|| error!(CoreError::ValueOverflow))?;
            if value != 0 {
                treasury_value = treasury_value
                    .checked_add(value)
                    .ok_or_else(|| error!(CoreError::ValueOverflow))?;
            }
        }

        let total_vaule = treasury_value
            .checked_add(gt_bank_value)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;
        let buyback_factor = self.config.load()?.buyback_factor();
        let max_buyback_value = apply_factor::<_, { gmsol_store::constants::MARKET_DECIMALS }>(
            &total_vaule,
            &buyback_factor,
        )
        .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        Ok(BuybackValue::new(gt_bank_value, max_buyback_value))
    }

    /// Reserve the final GT bank balances eligible for buyback.
    /// # Note
    /// We do not actually execute token transfers; instead, we only update
    /// the token balances recorded in the GT bank. This is because tokens exceeding
    /// the recorded amount can be transferred to the treasury bank at any time.
    fn update_balances(
        &self,
        vaults: &[AccountInfo<'info>],
        buyback_amount: u64,
    ) -> Result<(u128, Option<u128>)> {
        if buyback_amount == 0 {
            self.gt_bank.load_mut()?.record_all_transferred_out()?;
            return Ok((0, None));
        }

        let BuybackValue {
            max_buyback_value,
            gt_bank_value,
        } = self.get_max_buyback_value(vaults)?;

        let buyback_amount = u128::from(buyback_amount);

        let estimated_buyback_price = max_buyback_value
            .checked_div(buyback_amount)
            .ok_or_else(|| error!(CoreError::Internal))?;

        let max_buyback_price = self.store.load()?.gt().minting_cost();

        let buyback_price = estimated_buyback_price.min(max_buyback_price);
        let buyback_value = buyback_amount
            .checked_mul(buyback_price)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        if buyback_value == 0 {
            self.gt_bank.load_mut()?.record_all_transferred_out()?;
            return Ok((0, None));
        }

        msg!(
            "[Treasury] will spend {} to buyback {} (unit) GT at a price of {}",
            buyback_value,
            buyback_amount,
            buyback_price,
        );

        // Reserve balances for buyback.
        self.gt_bank
            .load_mut()?
            .reserve_balances(&buyback_value, &gt_bank_value)?;

        Ok((buyback_value, Some(buyback_price)))
    }
}

struct BuybackValue {
    gt_bank_value: u128,
    max_buyback_value: u128,
}

impl BuybackValue {
    fn new(gt_bank_value: u128, max_buyback_value: u128) -> Self {
        Self {
            gt_bank_value,
            max_buyback_value: gt_bank_value.min(max_buyback_value),
        }
    }
}
