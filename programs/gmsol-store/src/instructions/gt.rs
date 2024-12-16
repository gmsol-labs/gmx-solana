use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::{
    states::{
        gt::{GtExchange, GtExchangeVault, GtVesting},
        user::UserHeader,
        Seed, Store,
    },
    utils::internal,
    CoreError,
};

/// The accounts defintions for the [`initialize_gt`](crate::gmsol_store::initialize_gt) instruction.
#[derive(Accounts)]
pub struct InitializeGt<'info> {
    /// Authority
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    pub system_program: Program<'info, System>,
}

/// CHECK: only MARKET_KEEPER is allowed to use this instruction.
pub(crate) fn unchecked_initialize_gt(
    ctx: Context<InitializeGt>,
    decimals: u8,
    initial_minting_cost: u128,
    grow_factor: u128,
    grow_step: u64,
    ranks: &[u64],
) -> Result<()> {
    ctx.accounts.initialize_gt_state(
        decimals,
        initial_minting_cost,
        grow_factor,
        grow_step,
        ranks,
    )?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeGt<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> InitializeGt<'info> {
    fn initialize_gt_state(
        &self,
        decimals: u8,
        initial_minting_cost: u128,
        grow_factor: u128,
        grow_step: u64,
        ranks: &[u64],
    ) -> Result<()> {
        let mut store = self.store.load_mut()?;
        store.gt_mut().init(
            decimals,
            initial_minting_cost,
            grow_factor,
            grow_step,
            ranks,
        )?;
        Ok(())
    }
}

/// The accounts defintions for GT configuration instructions.
#[derive(Accounts)]
pub struct ConfigurateGt<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
}

impl<'info> internal::Authentication<'info> for ConfigurateGt<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// CHECK: only MARKET_KEEPER is authorized to use this instruction.
pub(crate) fn unchecked_gt_set_order_fee_discount_factors(
    ctx: Context<ConfigurateGt>,
    factors: &[u128],
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .gt_mut()
        .set_order_fee_discount_factors(factors)
}

/// CHECK: only GT_CONTROLLER is authorized to use this instruction.
pub(crate) fn unchecked_gt_set_referral_reward_factors(
    ctx: Context<ConfigurateGt>,
    factors: &[u128],
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .gt_mut()
        .set_referral_reward_factors(factors)
}

/// CHECK: only GT_CONTROLLER is authorized to use this instruction.
pub(crate) fn unchecked_gt_set_es_receiver_factor(
    ctx: Context<ConfigurateGt>,
    factor: u128,
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .gt_mut()
        .set_es_recevier_factor(factor)
}

/// CHECK: only GT_CONTROLLER is authorized to use this instruction.
pub(crate) fn unchecked_gt_set_exchange_time_window(
    ctx: Context<ConfigurateGt>,
    window: u32,
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .gt_mut()
        .set_exchange_time_window(window)
}

/// CHECK: only GT_CONTROLLER is authorized to use this instruction.
pub(crate) fn unchecked_gt_set_receiver(
    ctx: Context<ConfigurateGt>,
    receiver: &Pubkey,
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .gt_mut()
        .set_receiver(receiver)
}

/// The accounts definition for [`prepare_gt_exchange_vault`](crate::gmsol_store::prepare_gt_exchange_vault) instruction.
#[derive(Accounts)]
#[instruction(time_window_index: i64)]
pub struct PrepareGtExchangeVault<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet)]
    pub store: AccountLoader<'info, Store>,
    #[account(
        init_if_needed,
        space = 8 + GtExchangeVault::INIT_SPACE,
        payer = payer,
        seeds = [GtExchangeVault::SEED, store.key().as_ref(), &time_window_index.to_be_bytes()],
        bump,
    )]
    pub vault: AccountLoader<'info, GtExchangeVault>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn prepare_gt_exchange_vault(
    ctx: Context<PrepareGtExchangeVault>,
    time_window_index: i64,
    time_window: u32,
) -> Result<()> {
    let store = ctx.accounts.store.load()?;
    require_eq!(
        store.gt().exchange_time_window(),
        time_window,
        CoreError::InvalidArgument
    );

    match ctx.accounts.vault.load_init() {
        Ok(mut vault) => {
            vault.init(ctx.bumps.vault, &ctx.accounts.store.key(), time_window)?;
            drop(vault);
            ctx.accounts.vault.exit(&crate::ID)?;
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

    // Validate the vault.
    {
        let vault = ctx.accounts.vault.load()?;
        require!(vault.is_initialized(), CoreError::PreconditionsAreNotMet);
        require_eq!(
            vault.store,
            ctx.accounts.store.key(),
            CoreError::StoreMismatched
        );
        require_eq!(
            vault.time_window_index(),
            time_window_index,
            CoreError::InvalidArgument
        );
        require_eq!(
            vault.time_window(),
            time_window as i64,
            CoreError::InvalidArgument
        );
    }

    Ok(())
}

/// The accounts definition for [`request_gt_exchange`](crate::gmsol_store::request_gt_exchange) instruction.
#[derive(Accounts)]
pub struct RequestGtExchange<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    #[account(
        mut,
        constraint = vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        has_one = store,
        seeds = [GtExchangeVault::SEED, store.key().as_ref(), &vault.load()?.time_window_index().to_be_bytes()],
        bump = vault.load()?.bump,
    )]
    pub vault: AccountLoader<'info, GtExchangeVault>,
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + GtExchange::INIT_SPACE,
        seeds = [GtExchange::SEED, vault.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub exchange: AccountLoader<'info, GtExchange>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn request_gt_exchange(ctx: Context<RequestGtExchange>, amount: u64) -> Result<()> {
    let accounts = ctx.accounts;

    accounts.validate_and_init_exchange_if_needed(ctx.bumps.exchange)?;

    let mut store = accounts.store.load_mut()?;
    let mut vault = accounts.vault.load_mut()?;
    let mut user = accounts.user.load_mut()?;
    let mut exchange = accounts.exchange.load_mut()?;

    store
        .gt_mut()
        .unchecked_request_exchange(&mut user, &mut vault, &mut exchange, amount)?;

    Ok(())
}

impl<'info> RequestGtExchange<'info> {
    fn validate_and_init_exchange_if_needed(&mut self, bump: u8) -> Result<()> {
        match self.exchange.load_init() {
            Ok(mut exchange) => {
                exchange.init(
                    bump,
                    &self.owner.key(),
                    &self.store.key(),
                    &self.vault.key(),
                )?;
                drop(exchange);
                self.exchange.exit(&crate::ID)?;
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
        require!(
            self.exchange.load()?.is_initialized(),
            CoreError::PreconditionsAreNotMet
        );
        require_eq!(
            *self.exchange.load()?.owner(),
            self.owner.key(),
            CoreError::OwnerMismatched,
        );
        require_eq!(
            *self.exchange.load()?.store(),
            self.store.key(),
            CoreError::StoreMismatched,
        );
        Ok(())
    }
}

/// The accounts definition for [`confirm_gt_exchange_vault`](crate::confirm_gt_exchange_vault) instruction.
#[derive(Accounts)]
pub struct ConfirmGtExchangeVault<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    #[account(
        mut,
        constraint = vault.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = store,
        seeds = [GtExchangeVault::SEED, store.key().as_ref(), &vault.load()?.time_window_index().to_be_bytes()],
        bump = vault.load()?.bump,
    )]
    pub vault: AccountLoader<'info, GtExchangeVault>,
}

/// CHECK: only GT_CONTROLLER is authorized to use this instruction.
pub(crate) fn unchecked_confirm_gt_exchange_vault(
    ctx: Context<ConfirmGtExchangeVault>,
) -> Result<()> {
    let mut store = ctx.accounts.store.load_mut()?;
    let mut vault = ctx.accounts.vault.load_mut()?;
    store
        .gt_mut()
        .unchecked_confirm_exchange_vault(&mut vault)?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for ConfirmGtExchangeVault<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`close_gt_exchange`](crate::close_gt_exchange) instruction.
#[derive(Accounts)]
pub struct CloseGtExchange<'info> {
    pub authority: Signer<'info>,
    #[account(
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// CHECK: only used to receive the funds.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        constraint = vault.load()?.is_initialized() @ CoreError::InvalidArgument,
        constraint = vault.load()?.is_confirmed() @ CoreError::PreconditionsAreNotMet,
        has_one = store,
        seeds = [GtExchangeVault::SEED, store.key().as_ref(), &vault.load()?.time_window_index().to_be_bytes()],
        bump = vault.load()?.bump,
    )]
    pub vault: AccountLoader<'info, GtExchangeVault>,
    #[account(
        mut,
        close = owner,
        constraint = exchange.load()?.is_initialized() @ CoreError::InvalidArgument,
        has_one = store,
        has_one = owner,
        has_one = vault,
        seeds = [GtExchange::SEED, vault.key().as_ref(), owner.key().as_ref()],
        bump = exchange.load()?.bump,
    )]
    pub exchange: AccountLoader<'info, GtExchange>,
}

/// CHECK: only GT_CONTROLLER is allowed to use this instruction.
pub(crate) fn unchecked_close_gt_exchange(ctx: Context<CloseGtExchange>) -> Result<()> {
    let vault = ctx.accounts.vault.load()?;
    let exchange = ctx.accounts.exchange.load()?;
    msg!(
        "[GT] Closing confirmed exchange: vault_index = {}, vault = {}, owner = {}, amount = {}",
        vault.time_window_index(),
        exchange.vault(),
        exchange.owner(),
        exchange.amount()
    );
    Ok(())
}

impl<'info> internal::Authentication<'info> for CloseGtExchange<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`claim_es_gt`](crate::gmsol_store::claim_es_gt).
#[derive(Accounts)]
pub struct ClaimEsGt<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
}

pub(crate) fn claim_es_gt(ctx: Context<ClaimEsGt>) -> Result<()> {
    let accounts = ctx.accounts;
    let mut store = accounts.store.load_mut()?;
    let mut user = accounts.user.load_mut()?;
    store.gt_mut().unchecked_sync_es_factor(&mut user)
}

/// The accounts definition for [`request_gt_vesting`](crate::gmsol_store::request_gt_vesting).
#[derive(Accounts)]
pub struct RequestGtVesting<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + GtVesting::INIT_SPACE,
        seeds = [GtVesting::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub vesting: AccountLoader<'info, GtVesting>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn request_gt_vesting(ctx: Context<RequestGtVesting>, amount: u64) -> Result<()> {
    let accounts = ctx.accounts;
    accounts.validate_and_init_vesting_if_needed(ctx.bumps.vesting)?;

    msg!("Vesting account is initialized");

    if amount != 0 {
        let mut store = accounts.store.load_mut()?;
        let mut user = accounts.user.load_mut()?;
        let mut vesting = accounts.vesting.load_mut()?;
        store
            .gt_mut()
            .unchecked_request_vesting(&mut user, &mut vesting, amount)?;
    }
    Ok(())
}

impl<'info> RequestGtVesting<'info> {
    fn validate_and_init_vesting_if_needed(&mut self, bump: u8) -> Result<()> {
        match self.vesting.load_init() {
            Ok(mut exchange) => {
                let (divisor, time_window) = {
                    let store = self.store.load()?;
                    (
                        store.gt().es_vesting_divisor(),
                        store.gt().exchange_time_window(),
                    )
                };
                exchange.init(
                    bump,
                    &self.owner.key(),
                    &self.store.key(),
                    divisor,
                    time_window,
                )?;
                drop(exchange);
                self.vesting.exit(&crate::ID)?;
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
        require!(
            self.vesting.load()?.is_initialized(),
            CoreError::PreconditionsAreNotMet
        );
        require_eq!(
            *self.vesting.load()?.owner(),
            self.owner.key(),
            CoreError::OwnerMismatched,
        );
        require_eq!(
            *self.vesting.load()?.store(),
            self.store.key(),
            CoreError::StoreMismatched,
        );
        Ok(())
    }
}

/// The accounts definition for [`update_gt_vesting`](crate::gmsol_store::update_gt_vesting).
#[derive(Accounts)]
pub struct UpdateGtVesting<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    #[account(
        mut,
        constraint = vesting.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [GtVesting::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = vesting.load()?.bump,
    )]
    /// Vesting.
    pub vesting: AccountLoader<'info, GtVesting>,
}

pub(crate) fn update_gt_vesting(ctx: Context<UpdateGtVesting>) -> Result<()> {
    let accounts = ctx.accounts;
    let mut store = accounts.store.load_mut()?;
    let mut user = accounts.user.load_mut()?;
    let mut vesting = accounts.vesting.load_mut()?;
    store
        .gt_mut()
        .unchecked_update_vesting(&mut user, &mut vesting)?;
    Ok(())
}

/// The accounts definition for [`close_gt_vesting`](crate::gmsol_store::close_gt_vesting).
#[derive(Accounts)]
pub struct CloseGtVesting<'info> {
    pub owner: Signer<'info>,
    #[account(
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    #[account(
        mut,
        close = owner,
        constraint = vesting.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [GtVesting::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = vesting.load()?.bump,
    )]
    pub vesting: AccountLoader<'info, GtVesting>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn close_gt_vesting(ctx: Context<CloseGtVesting>) -> Result<()> {
    let accounts = ctx.accounts;
    require!(
        accounts.vesting.load()?.is_empty(),
        CoreError::PreconditionsAreNotMet
    );
    require_eq!(
        accounts.user.load()?.gt.vesting_es_amount,
        0,
        CoreError::Internal
    );
    Ok(())
}

/// The accounts definition for [`claim_es_gt_vault_by_vesting`](crate::gmsol_store::claim_es_gt_vault_via_vesting).
#[derive(Accounts)]
pub struct ClaimEsGtVaultViaVesting<'info> {
    /// The owner.
    pub owner: Signer<'info>,
    #[account(
        mut,
        constraint = store.load()?.gt().is_initialized() @ CoreError::PreconditionsAreNotMet,
    )]
    pub store: AccountLoader<'info, Store>,
    /// User account.
    #[account(
        mut,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    #[account(
        mut,
        constraint = vesting.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [GtVesting::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = vesting.load()?.bump,
    )]
    /// Vesting.
    pub vesting: AccountLoader<'info, GtVesting>,
}

pub(crate) fn claim_es_gt_vault_via_vesting(
    ctx: Context<ClaimEsGtVaultViaVesting>,
    amount: u64,
) -> Result<()> {
    let accounts = ctx.accounts;

    let mut store = accounts.store.load_mut()?;

    store.gt().validate_receiver(&accounts.owner.key())?;

    let mut user = accounts.user.load_mut()?;
    let mut vesting = accounts.vesting.load_mut()?;

    store
        .gt_mut()
        .unchecked_distribute_es_vault(&mut user, &mut vesting, amount)?;

    Ok(())
}
