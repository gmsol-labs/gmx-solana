use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Token, TokenAccount, TransferChecked};
use anchor_spl::token_interface::Mint;

use crate::{
    events::{BuilderFeeClaimed, BuilderFeeSettled, EventEmitter},
    states::{builder_fee::BuilderFeeTokenController, user::UserHeader, Order, Seed, Store},
    utils::internal,
    CoreError,
};

use gmsol_utils::InitSpace;

/// The accounts definition for
/// [`initialize_builder_fee_token_controller`](crate::gmsol_store::initialize_builder_fee_token_controller).
#[derive(Accounts)]
pub struct InitializeBuilderFeeTokenController<'info> {
    /// The caller.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The token mint to initialize the controller for.
    pub token_mint: InterfaceAccount<'info, Mint>,
    /// The controller account to initialize.
    #[account(
        init,
        payer = authority,
        space = 8 + BuilderFeeTokenController::INIT_SPACE,
        seeds = [
            BuilderFeeTokenController::SEED,
            store.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub controller: AccountLoader<'info, BuilderFeeTokenController>,
    /// System program.
    pub system_program: Program<'info, System>,
}

/// Initialize the per-token access control account for builder fees.
///
/// ## CHECK
/// - Only MARKET_KEEPER can initialize the controller.
pub(crate) fn unchecked_initialize_builder_fee_token_controller(
    ctx: Context<InitializeBuilderFeeTokenController>,
) -> Result<()> {
    ctx.accounts.controller.load_init()?.init(
        &ctx.accounts.store.key(),
        &ctx.accounts.token_mint.key(),
        ctx.bumps.controller,
    );
    Ok(())
}

impl<'info> internal::Authentication<'info> for InitializeBuilderFeeTokenController<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

/// The accounts definition for [`settle_builder_fee`](crate::gmsol_store::settle_builder_fee).
#[event_cpi]
#[derive(Accounts)]
pub struct SettleBuilderFee<'info> {
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The order whose builder fee is to be settled.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
    /// The builder's User Account.
    #[account(
        has_one = store,
        constraint = builder_user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
    )]
    pub builder_user: AccountLoader<'info, UserHeader>,
    /// The token the builder fee is denominated in (the collateral token of
    /// the order).
    pub collateral_token: InterfaceAccount<'info, Mint>,
    /// The order's escrow account for the collateral token.
    #[account(
        mut,
        token::mint = collateral_token,
        token::authority = order,
    )]
    pub escrow: Account<'info, TokenAccount>,
    /// The builder's claim vault: the associated token account of the
    /// collateral token owned by the builder's User Account.
    #[account(
        mut,
        associated_token::mint = collateral_token,
        associated_token::authority = builder_user,
    )]
    pub claim_vault: Account<'info, TokenAccount>,
    /// Token program.
    pub token_program: Program<'info, Token>,
}

/// Settle the builder fee of the given order.
pub(crate) fn settle_builder_fee(ctx: Context<SettleBuilderFee>) -> Result<()> {
    let amount = ctx.accounts.order.load()?.builder_fee_amount();

    // An amount of zero is an explicit no-op.
    if amount == 0 {
        return Ok(());
    }

    {
        let order = ctx.accounts.order.load()?;

        // A non-zero amount implies that the builder must have been set.
        let builder = order.builder().ok_or_else(|| error!(CoreError::Internal))?;
        require_keys_eq!(
            *builder,
            ctx.accounts.builder_user.key(),
            CoreError::InvalidUserAccount
        );

        let collateral_token = order.params.collateral_token;
        require_keys_eq!(
            ctx.accounts.collateral_token.key(),
            collateral_token,
            CoreError::TokenMintMismatched
        );

        let long_token = order.tokens.long_token();
        let short_token = order.tokens.short_token();
        let expected_escrow = if long_token.token() == Some(collateral_token) {
            long_token.account()
        } else if short_token.token() == Some(collateral_token) {
            short_token.account()
        } else {
            None
        }
        .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
        require_keys_eq!(
            ctx.accounts.escrow.key(),
            expected_escrow,
            CoreError::TokenAccountNotProvided
        );
    }

    let signer = ctx.accounts.order.load()?.signer();
    let seeds = signer.as_seeds();
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.escrow.to_account_info(),
                mint: ctx.accounts.collateral_token.to_account_info(),
                to: ctx.accounts.claim_vault.to_account_info(),
                authority: ctx.accounts.order.to_account_info(),
            },
        )
        .with_signer(&[&seeds]),
        amount,
        ctx.accounts.collateral_token.decimals,
    )?;

    ctx.accounts.order.load_mut()?.builder_fee_amount = 0;

    EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority).emit_cpi(
        &BuilderFeeSettled::new(
            &ctx.accounts.store.key(),
            &ctx.accounts.order.key(),
            &ctx.accounts.builder_user.key(),
            &ctx.accounts.collateral_token.key(),
            amount,
        )?,
    )?;

    Ok(())
}

/// The accounts definition for [`claim_builder_fees`](crate::gmsol_store::claim_builder_fees).
#[event_cpi]
#[derive(Accounts)]
pub struct ClaimBuilderFees<'info> {
    /// The owner of the builder's User Account.
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The builder's User Account.
    #[account(
        has_one = store,
        has_one = owner,
        constraint = builder_user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = builder_user.load()?.bump,
    )]
    pub builder_user: AccountLoader<'info, UserHeader>,
    /// The per-token access control account.
    ///
    /// Currently it is only required to exist: it is the hook for future
    /// access control on builder fee withdrawal (e.g. a withdrawal timelock).
    #[account(
        seeds = [
            BuilderFeeTokenController::SEED,
            store.key().as_ref(),
            token.key().as_ref(),
        ],
        bump = controller.load()?.bump,
    )]
    pub controller: AccountLoader<'info, BuilderFeeTokenController>,
    /// The token to claim.
    pub token: InterfaceAccount<'info, Mint>,
    /// The builder's claim vault: the associated token account of the token
    /// owned by the builder's User Account.
    #[account(
        mut,
        associated_token::mint = token,
        associated_token::authority = builder_user,
    )]
    pub claim_vault: Account<'info, TokenAccount>,
    /// The destination token account.
    #[account(mut, token::mint = token)]
    pub receiver_vault: Account<'info, TokenAccount>,
    /// Token program.
    pub token_program: Program<'info, Token>,
}

/// Claim the settled builder fees from the claim vault.
pub(crate) fn claim_builder_fees(ctx: Context<ClaimBuilderFees>) -> Result<()> {
    let amount = ctx.accounts.claim_vault.amount;

    // Nothing to claim is an explicit no-op.
    if amount == 0 {
        return Ok(());
    }

    let store = ctx.accounts.store.key();
    let owner = ctx.accounts.owner.key();
    let bump_bytes = [ctx.accounts.builder_user.load()?.bump];
    let seeds: [&[u8]; 4] = [
        UserHeader::SEED,
        store.as_ref(),
        owner.as_ref(),
        &bump_bytes,
    ];
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.claim_vault.to_account_info(),
                mint: ctx.accounts.token.to_account_info(),
                to: ctx.accounts.receiver_vault.to_account_info(),
                authority: ctx.accounts.builder_user.to_account_info(),
            },
        )
        .with_signer(&[&seeds]),
        amount,
        ctx.accounts.token.decimals,
    )?;

    EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority).emit_cpi(
        &BuilderFeeClaimed::new(
            &store,
            &ctx.accounts.builder_user.key(),
            &ctx.accounts.token.key(),
            &ctx.accounts.receiver_vault.key(),
            amount,
        )?,
    )?;

    Ok(())
}
