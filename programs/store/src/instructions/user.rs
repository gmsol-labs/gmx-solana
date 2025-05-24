use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::{
    states::{
        user::{ReferralCodeBytes, ReferralCodeV2, UserHeader},
        Seed, Store,
    },
    CoreError,
};

/// The accounts definitions for [`prepare_user`](crate::gmsol_store::prepare_user) instruction.
#[derive(Accounts)]
pub struct PrepareUser<'info> {
    /// Owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + UserHeader::space(0),
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn prepare_user(ctx: Context<PrepareUser>) -> Result<()> {
    let store = ctx.accounts.store.key();
    let owner = ctx.accounts.owner.key;
    {
        match ctx.accounts.user.load_init() {
            Ok(mut user) => {
                user.init(&store, owner, ctx.bumps.user)?;
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
    }
    ctx.accounts.user.exit(&crate::ID)?;
    {
        let user = ctx.accounts.user.load()?;
        require!(user.is_initialized(), CoreError::InvalidUserAccount);
        require_keys_eq!(user.store, store, CoreError::InvalidUserAccount);
        require_keys_eq!(user.owner, *owner, CoreError::InvalidUserAccount);
        require_eq!(user.bump, ctx.bumps.user, CoreError::InvalidUserAccount);
    }
    Ok(())
}

/// The accounts definition for [`initialize_referral_code`](crate::gmsol_store::initialize_referral_code)
/// instruction.
#[derive(Accounts)]
#[instruction(code: [u8; 8])]
pub struct InitializeReferralCode<'info> {
    /// Owner.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Referral Code Account.
    #[account(
        init,
        payer = owner,
        space = 8 + ReferralCodeV2::INIT_SPACE,
        seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &code],
        bump,
    )]
    pub referral_code: AccountLoader<'info, ReferralCodeV2>,
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
    pub system_program: Program<'info, System>,
}

pub(crate) fn initialize_referral_code(
    ctx: Context<InitializeReferralCode>,
    code: ReferralCodeBytes,
) -> Result<()> {
    require!(
        code != ReferralCodeBytes::default(),
        CoreError::InvalidArgument
    );

    // Initialize Referral Code Account.
    ctx.accounts.referral_code.load_init()?.init(
        ctx.bumps.referral_code,
        code,
        &ctx.accounts.store.key(),
        ctx.accounts.owner.key,
    );

    // Set referral code address.
    ctx.accounts
        .user
        .load_mut()?
        .referral
        .set_code(&ctx.accounts.referral_code.key())?;
    Ok(())
}

/// The accounts definitions for [`set_referrer`](crate::gmsol_store::set_referrer)
/// instruction.
#[derive(Accounts)]
#[instruction(code: [u8; 8])]
pub struct SetReferrer<'info> {
    pub owner: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        has_one = owner,
        has_one = store,
        constraint = user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    /// Referral Code Account.
    #[account(
        has_one = store,
        constraint = referral_code.load()?.code == code @ CoreError::ReferralCodeMismatched,
        seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &code],
        bump = referral_code.load()?.bump,
    )]
    pub referral_code: AccountLoader<'info, ReferralCodeV2>,
    /// Referrer.
    #[account(
        mut,
        has_one = store,
        constraint = referrer_user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        constraint = referrer_user.load()?.owner == referral_code.load()?.owner @ CoreError::OwnerMismatched,
        constraint = referrer_user.load()?.referral.code == referral_code.key() @ CoreError::ReferralCodeMismatched,
        constraint = referrer_user.key() != user.key() @ CoreError::SelfReferral,
        seeds = [UserHeader::SEED, store.key().as_ref(), referrer_user.load()?.owner.as_ref()],
        bump = referrer_user.load()?.bump,
    )]
    pub referrer_user: AccountLoader<'info, UserHeader>,
}

pub(crate) fn set_referrer(ctx: Context<SetReferrer>, _code: ReferralCodeBytes) -> Result<()> {
    require!(
        ctx.accounts.referrer_user.load()?.referral.referrer != ctx.accounts.user.load()?.owner,
        CoreError::MutualReferral
    );
    ctx.accounts
        .user
        .load_mut()?
        .referral
        .set_referrer(&mut *ctx.accounts.referrer_user.load_mut()?)?;
    Ok(())
}

/// The accounts definitions for [`accept_referral_code`](crate::gmsol_store::accept_referral_code) instruction.
#[derive(Accounts)]
pub struct AcceptReferralCode<'info> {
    pub next_owner: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        mut,
        has_one = store,
        constraint = user.load()?.owner == referral_code.load()?.owner @ CoreError::OwnerMismatched,
        constraint = user.load()?.referral.code == referral_code.key() @ CoreError::ReferralCodeMismatched,
        seeds = [UserHeader::SEED, store.key().as_ref(), referral_code.load()?.owner.as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    /// Referral Code Account.
    #[account(
        mut,
        has_one = store,
        seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &referral_code.load()?.code],
        bump = referral_code.load()?.bump,
    )]
    pub referral_code: AccountLoader<'info, ReferralCodeV2>,
    /// Receiver.
    #[account(
        mut,
        has_one = store,
        constraint = receiver_user.load()?.owner == next_owner.key() @ CoreError::OwnerMismatched,
        constraint = receiver_user.key() != user.key() @ CoreError::SelfReferral,
        seeds = [UserHeader::SEED, store.key().as_ref(), next_owner.key().as_ref()],
        bump = receiver_user.load()?.bump,
    )]
    pub receiver_user: AccountLoader<'info, UserHeader>,
}

pub(crate) fn accept_referral_code(ctx: Context<AcceptReferralCode>) -> Result<()> {
    let mut code = ctx.accounts.referral_code.load_mut()?;
    let mut receiver_user = ctx.accounts.receiver_user.load_mut()?;
    ctx.accounts
        .user
        .load_mut()?
        .unchecked_complete_code_transfer(&mut code, &mut receiver_user)?;

    msg!(
        "[Referral] the owner of referral code `{:?}` is now {}",
        code.code,
        code.owner,
    );

    Ok(())
}

/// The accounts definitions for [`transfer_referral_code`](crate::gmsol_store::transfer_referral_code) instruction.
#[derive(Accounts)]
pub struct TransferReferralCode<'info> {
    pub owner: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        has_one = owner,
        has_one = store,
        constraint = user.load()?.owner == referral_code.load()?.owner @ CoreError::OwnerMismatched,
        constraint = user.load()?.referral.code == referral_code.key() @ CoreError::ReferralCodeMismatched,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    /// Referral Code Account.
    #[account(
        mut,
        has_one = store,
        seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &referral_code.load()?.code],
        bump = referral_code.load()?.bump,
    )]
    pub referral_code: AccountLoader<'info, ReferralCodeV2>,
    /// Receiver.
    #[account(
        has_one = store,
        constraint = receiver_user.key() != user.key() @ CoreError::SelfReferral,
        seeds = [UserHeader::SEED, store.key().as_ref(), receiver_user.load()?.owner.as_ref()],
        bump = receiver_user.load()?.bump,
    )]
    pub receiver_user: AccountLoader<'info, UserHeader>,
}

pub(crate) fn transfer_referral_code(ctx: Context<TransferReferralCode>) -> Result<()> {
    let mut code = ctx.accounts.referral_code.load_mut()?;
    let receiver_user = ctx.accounts.receiver_user.load()?;
    ctx.accounts
        .user
        .load()?
        .unchecked_transfer_code(&mut code, &receiver_user)?;

    msg!(
        "[Referral] the next owner of referral code `{:?}` is now {}",
        code.code,
        code.next_owner(),
    );

    Ok(())
}

/// The accounts definitions for [`cancel_referral_code_transfer`](crate::gmsol_store::cancel_referral_code_transfer) instruction.
#[derive(Accounts)]
pub struct CancelReferralCodeTransfer<'info> {
    pub owner: Signer<'info>,
    pub store: AccountLoader<'info, Store>,
    /// User Account.
    #[account(
        has_one = owner,
        has_one = store,
        constraint = user.load()?.owner == referral_code.load()?.owner @ CoreError::OwnerMismatched,
        constraint = user.load()?.referral.code == referral_code.key() @ CoreError::ReferralCodeMismatched,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user.load()?.bump,
    )]
    pub user: AccountLoader<'info, UserHeader>,
    /// Referral Code Account.
    #[account(
        mut,
        has_one = store,
        seeds = [ReferralCodeV2::SEED, store.key().as_ref(), &referral_code.load()?.code],
        bump = referral_code.load()?.bump,
    )]
    pub referral_code: AccountLoader<'info, ReferralCodeV2>,
}

pub(crate) fn cancel_referral_code_transfer(
    ctx: Context<CancelReferralCodeTransfer>,
) -> Result<()> {
    let mut code = ctx.accounts.referral_code.load_mut()?;
    code.set_next_owner(ctx.accounts.owner.key)?;
    msg!(
        "[Referral] the next owner of referral code `{:?}` is now {}",
        code.code,
        code.next_owner(),
    );

    Ok(())
}
