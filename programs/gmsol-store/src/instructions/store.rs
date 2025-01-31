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
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The authority of the the [`Store`] account.
    ///
    /// If it is not specified, the `payer` will be set as the authority of this [`Store`] Account.
    pub authority: Option<Signer<'info>>,
    /// The receiver address of the the [`Store`] account.
    ///
    /// Defaults to the authority address.
    pub receiver: Option<Signer<'info>>,
    /// The holding address.
    ///
    /// Defaults to the authority address.
    pub holding: Option<Signer<'info>>,
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

pub(crate) fn initialize(ctx: Context<Initialize>, key: String) -> Result<()> {
    ctx.accounts.validate_key(&key)?;

    let mut store = ctx.accounts.store.load_init()?;
    let authority = ctx
        .accounts
        .authority
        .as_ref()
        .map(|a| a.key())
        .unwrap_or(ctx.accounts.payer.key());
    let receiver = ctx
        .accounts
        .receiver
        .as_ref()
        .map(|a| a.key())
        .unwrap_or(authority);
    let holding = ctx
        .accounts
        .holding
        .as_ref()
        .map(|a| a.key())
        .unwrap_or(authority);
    store.init(authority, &key, ctx.bumps.store, receiver, holding)?;
    Ok(())
}

impl Initialize<'_> {
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
    /// Next authority address.
    /// CHECK: only the address is used.
    pub next_authority: UncheckedAccount<'info>,
}

/// Transfer the authority of the store to a new one.
///
/// ## CHECK
/// - Only ADMIN can execute this instruction.
pub(crate) fn unchecked_transfer_store_authority(
    ctx: Context<TransferStoreAuthority>,
) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .set_next_authority(ctx.accounts.next_authority.key)?;
    msg!(
        "[Store] the next_authority is now {}",
        ctx.accounts.next_authority.key
    );
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

/// The accounts definition for
/// [`accept_store_authority`](crate::gmsol_store::accept_store_authority).
#[derive(Accounts)]
pub struct AcceptStoreAuthority<'info> {
    /// The next authority.
    pub next_authority: Signer<'info>,
    /// The store account whose authority is being transferred.
    #[account(mut, has_one = next_authority)]
    pub store: AccountLoader<'info, Store>,
}

pub(crate) fn accept_store_authority(ctx: Context<AcceptStoreAuthority>) -> Result<()> {
    let authority = ctx.accounts.store.load_mut()?.update_authority()?;
    msg!("[Store] the authority is now {}", authority);
    Ok(())
}

/// The accounts definition for [`transfer_receiver`](crate::gmsol_store::transfer_receiver).
#[derive(Accounts)]
pub struct TransferReceiver<'info> {
    /// The caller of this instruction.
    #[account(
        constraint = authority.key() == store.load()?.receiver() @ CoreError::PermissionDenied,
    )]
    pub authority: Signer<'info>,
    /// The store account whose receiver is to be transferred.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// The new receiver.
    /// CHECK: only the address is used.
    pub next_receiver: UncheckedAccount<'info>,
}

pub(crate) fn transfer_receiver(ctx: Context<TransferReceiver>) -> Result<()> {
    ctx.accounts
        .store
        .load_mut()?
        .set_next_receiver(ctx.accounts.next_receiver.key)?;
    msg!(
        "[Treasury] the next_receiver is now {}",
        ctx.accounts.next_receiver.key
    );
    Ok(())
}

/// The accounts definition for [`accept_receiver`](crate::gmsol_store::accept_receiver).
#[derive(Accounts)]
pub struct AcceptReceiver<'info> {
    /// The next receiver.
    #[account(
        constraint = next_receiver.key() == store.load()?.next_receiver() @ CoreError::PermissionDenied,
    )]
    pub next_receiver: Signer<'info>,
    /// The store account whose receiver is being transferred.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
}

pub(crate) fn accept_receiver(ctx: Context<AcceptReceiver>) -> Result<()> {
    let receiver = ctx.accounts.store.load_mut()?.update_receiver()?;
    msg!("[Treasury] the receiver is now {}", receiver);
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
