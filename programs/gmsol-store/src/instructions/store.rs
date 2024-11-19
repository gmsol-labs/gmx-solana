use anchor_lang::prelude::*;
use gmsol_utils::{to_seed, InitSpace};

use crate::{
    states::{Seed, Store, TokenMapHeader},
    utils::internal,
    CoreError,
};

/// The accounts definition for [`initialize`](crate::gmsol_store::initialize).
#[derive(Accounts)]
#[instruction(key: String)]
pub struct Initialize<'info> {
    /// The payer for the rent-exempt fee of the [`Store`] Account.
    /// If `authority` is not specified, it will be set as the authority of this Store Account.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The account to be used for creating the [`Store`] Account.
    /// Its address is a PDA derived from a constant [`SEED`](Store::SEED)
    /// and a hashed key as the seeds.
    #[account(
        init,
        payer = payer,
        space = 8 + Store::INIT_SPACE,
        seeds = [Store::SEED, &to_seed(&key)],
        bump,
    )]
    pub store: AccountLoader<'info, Store>,
    /// The [`System`] program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn initialize(
    ctx: Context<Initialize>,
    key: String,
    authority: Option<Pubkey>,
) -> Result<()> {
    ctx.accounts.validate_key(&key)?;

    let mut store = ctx.accounts.store.load_init()?;
    store.init(
        authority.unwrap_or(ctx.accounts.payer.key()),
        &key,
        ctx.bumps.store,
    )?;
    Ok(())
}

impl<'info> Initialize<'info> {
    fn validate_key(&self, key: &str) -> Result<()> {
        #[cfg(not(feature = "multi-store"))]
        require!(key.is_empty(), CoreError::NonDefaultStore);

        msg!("initializing a new store with key = {}", key);
        Ok(())
    }
}

/// The accounts definition for
/// [`transfer_store_authority`](crate::gmsol_store::transfer_store_authority).
#[derive(Accounts)]
pub struct TransferStoreAuthority<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    /// The store account whose authority is to be transferred.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

/// Transfer the authority of the store to a new one.
///
/// ## CHECK
/// - Only ADMIN can execute this instruction.
pub(crate) fn unchecked_transfer_store_authority(
    ctx: Context<TransferStoreAuthority>,
    new_authority: Pubkey,
) -> Result<()> {
    require!(
        ctx.accounts.authority.key() != new_authority,
        CoreError::InvalidArgument
    );
    ctx.accounts.store.load_mut()?.authority = new_authority;
    Ok(())
}

impl<'info> internal::Authentication<'info> for TransferStoreAuthority<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`set_receiver`](crate::gmsol_store::set_receiver).
#[derive(Accounts)]
pub struct SetReceiver<'info> {
    /// The caller of this instruction.
    #[account(
        constraint = authority.key() == store.load()?.receiver() @ CoreError::PermissionDenied,
    )]
    pub authority: Signer<'info>,
    /// The store account whose authority is to be transferred.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// New receiver.
    /// CHECK: only the address is used.
    #[account(
        constraint = receiver.key() != authority.key() @ CoreError::PreconditionsAreNotMet,
    )]
    pub receiver: UncheckedAccount<'info>,
}

pub(crate) fn set_receiver(ctx: Context<SetReceiver>) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .unchecked_set_receiver(ctx.accounts.receiver.key)?;
    msg!(
        "[Treasury] the receiver is now {}",
        ctx.accounts.receiver.key
    );
    Ok(())
}

/// The accounts definition for [`set_token_map`](crate::gmsol_store::set_token_map).
#[derive(Accounts)]
pub struct SetTokenMap<'info> {
    /// The caller of this instruction.
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// Token map to use.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
}

/// Set token map.
///
/// ## Check
/// - Only MARKET_KEEPER can perform this action.
pub(crate) fn unchecked_set_token_map(ctx: Context<SetTokenMap>) -> Result<()> {
    ctx.accounts.store.load_mut()?.token_map = ctx.accounts.token_map.key();
    Ok(())
}

impl<'info> internal::Authentication<'info> for SetTokenMap<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

#[derive(Accounts)]
pub struct ReadStore<'info> {
    pub store: AccountLoader<'info, Store>,
}

/// Get the token map address of the store.
pub(crate) fn _get_token_map(ctx: Context<ReadStore>) -> Result<Option<Pubkey>> {
    Ok(ctx.accounts.store.load()?.token_map().copied())
}
