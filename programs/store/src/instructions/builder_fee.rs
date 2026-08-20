use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    events::{BuilderFeeSettled, EventEmitter},
    states::{user::UserHeader, Order, Store},
    CoreError,
};

/// The accounts definition for
/// [`settle_builder_fee`](crate::gmsol_store::settle_builder_fee).
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
    /// The final output token mint of the order, i.e. the token the
    /// builder fee is denominated in.
    pub final_output_token: Box<Account<'info, Mint>>,
    /// The order's escrow account for the final output token.
    #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
        constraint = order.load()?.tokens().final_output_token.account() == Some(escrow.key()) @ CoreError::TokenAccountMismatched,
    )]
    pub escrow: Box<Account<'info, TokenAccount>>,
    /// The builder's User Account.
    ///
    /// Only required when the order has a non-zero recorded builder fee
    /// amount. The no-op path (a recorded amount of zero, including
    /// orders that never had a builder set) performs no CPI and does not
    /// need it, so it may be omitted.
    #[account(
        has_one = store,
        constraint = builder_user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
    )]
    pub builder_user: Option<AccountLoader<'info, UserHeader>>,
    /// The builder's claim vault: the associated token account of the
    /// final output token owned by the builder's User Account.
    ///
    /// Required to already exist, so no `init_if_needed` is used here.
    /// Nothing currently guarantees that existence ahead of time: the
    /// instruction that is meant to (CON-40's `set_builder_fee`) has not
    /// landed yet. Until it does, a missing vault just makes settlement,
    /// and therefore closing an order with a non-zero fee, fail rather
    /// than misroute anything. Only required alongside `builder_user`,
    /// for the same reason.
    #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = builder_user,
    )]
    pub claim_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// Token program.
    pub token_program: Program<'info, Token>,
}

impl SettleBuilderFee<'_> {
    /// Settle the builder fee of the given order.
    ///
    /// Permissionless and idempotent: a recorded amount of zero, including
    /// orders that never had a builder set, is an explicit no-op that
    /// performs no token transfer, no state update, and no CPI. This makes
    /// it safe to call in any order state, before or after terminal.
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let recorded_amount = ctx.accounts.order.load()?.builder_fee_amount();
        if recorded_amount == 0 {
            return Ok(());
        }

        let builder_user = ctx
            .accounts
            .builder_user
            .as_ref()
            .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
        let claim_vault = ctx
            .accounts
            .claim_vault
            .as_ref()
            .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;

        {
            // A non-zero recorded amount implies a builder must have been set.
            let order = ctx.accounts.order.load()?;
            let builder = order.builder().ok_or_else(|| error!(CoreError::Internal))?;
            require_keys_eq!(*builder, builder_user.key(), CoreError::InvalidUserAccount);
        }

        // `claim_vault` is the ATA of (`builder_user`, `final_output_token`) per
        // the `associated_token` constraint above, so its owner and mint are
        // already guaranteed here.

        // Under the charging invariant the escrow always covers the recorded
        // amount, so this clamp should never actually reduce anything; it is
        // defense in depth, guaranteeing a protocol bug can never make an
        // order permanently unclosable. Any discrepancy is observable via the
        // two amounts recorded on the emitted event below.
        let settled_amount = recorded_amount.min(ctx.accounts.escrow.amount);

        let signer = ctx.accounts.order.load()?.signer();
        let seeds = signer.as_seeds();
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.escrow.to_account_info(),
                    mint: ctx.accounts.final_output_token.to_account_info(),
                    to: claim_vault.to_account_info(),
                    authority: ctx.accounts.order.to_account_info(),
                },
            )
            .with_signer(&[&seeds]),
            settled_amount,
            ctx.accounts.final_output_token.decimals,
        )?;

        ctx.accounts.order.load_mut()?.builder_fee_amount = 0;

        EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority).emit_cpi(
            &BuilderFeeSettled::new(
                &ctx.accounts.store.key(),
                &ctx.accounts.order.key(),
                &builder_user.key(),
                &ctx.accounts.final_output_token.key(),
                recorded_amount,
                settled_amount,
            )?,
        )?;

        Ok(())
    }
}
