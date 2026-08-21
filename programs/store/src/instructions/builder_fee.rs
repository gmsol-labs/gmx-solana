use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    events::{BuilderFeeClaimed, BuilderFeeSettled, EventEmitter},
    states::{
        user::{UserHeader, USER_TOKEN_CONTROLLER_SEED},
        Order, Seed, Store,
    },
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

/// The accounts definition for
/// [`claim_builder_fees`](crate::gmsol_store::claim_builder_fees).
#[event_cpi]
#[derive(Accounts)]
pub struct ClaimBuilderFees<'info> {
    /// The owner of the [`user_account`](Self::user_account). The only
    /// signer allowed to claim its builder fees.
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The builder's User Account.
    #[account(
        constraint = user_account.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        has_one = owner,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), owner.key().as_ref()],
        bump = user_account.load()?.bump,
    )]
    pub user_account: AccountLoader<'info, UserHeader>,
    /// The token mint the claim is denominated in.
    pub token_mint: Box<Account<'info, Mint>>,
    /// The claim vault: the associated token account of `token_mint`
    /// owned by `user_account`.
    ///
    /// Its authority is the program-controlled User Account PDA, so this
    /// instruction is the only path that can move tokens out of it.
    ///
    /// Must already exist when provided; this instruction never creates
    /// it. Omitting it is the no-op path: a builder with no vault for
    /// this mint has nothing to claim, and no transfer is attempted.
    ///
    /// Omitting it is a no-op even when the vault exists and holds a
    /// balance. The SDK always passes the derived address, so an
    /// omission can only ever be deliberate.
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = user_account,
    )]
    pub claim_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// The destination token account for `token_mint`, chosen by the
    /// owner. Not required to be an associated token account, but a
    /// transferring claim rejects the claim vault itself; see
    /// [`claim_builder_fees`](crate::gmsol_store::claim_builder_fees).
    #[account(mut, token::mint = token_mint)]
    pub destination: Box<Account<'info, TokenAccount>>,
    /// The (per-user, per-token) user token controller PDA.
    ///
    /// Reserved for future withdrawal access control (e.g. a timelock).
    /// It has no backing account and no data today, so the only check
    /// performed on it is that the passed address matches its PDA
    /// derivation; adding real behavior behind it later is not a
    /// breaking change to this instruction's accounts.
    /// CHECK: only the PDA derivation is verified, by the constraints below.
    #[account(
        seeds = [
            USER_TOKEN_CONTROLLER_SEED,
            user_account.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
    )]
    pub user_token_controller: UncheckedAccount<'info>,
    /// Token program.
    pub token_program: Program<'info, Token>,
}

impl ClaimBuilderFees<'_> {
    /// Claim the full balance of a builder's claim vault to an owner-chosen
    /// destination.
    ///
    /// Restricted to the [`user_account`](ClaimBuilderFees::user_account)'s
    /// owner: no other signer can initiate a claim, and no other instruction
    /// can move tokens out of a claim vault, since its authority is the
    /// program-controlled User Account PDA. Idempotent: having nothing to
    /// claim is an explicit no-op, performing no token transfer, no CPI, and
    /// emitting no event. That covers both an existing vault with a zero
    /// balance and a vault that was never created, the latter by omitting
    /// [`claim_vault`](ClaimBuilderFees::claim_vault) entirely. Not gated by
    /// the `BuilderFee` feature flag, since disabling the feature must never
    /// freeze already-settled funds.
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        // An omitted vault is the "never settled for this mint" case: there is
        // no account to hold a balance, so there is nothing to claim.
        let Some(claim_vault) = ctx.accounts.claim_vault.as_ref() else {
            return Ok(());
        };

        let amount = claim_vault.amount;
        if amount == 0 {
            return Ok(());
        }

        // Only claims that actually transfer are affected: spl-token
        // short-circuits a self-transfer to `Ok(())` after validation without
        // moving any balance, which would emit `BuilderFeeClaimed` for the
        // full vault balance while the vault still holds it. Checked here
        // rather than as an account constraint so that it cannot turn a
        // no-op, which moves nothing and emits nothing either way, into a
        // failure.
        require_keys_neq!(
            ctx.accounts.destination.key(),
            claim_vault.key(),
            CoreError::InvalidArgument
        );

        let store = ctx.accounts.store.key();
        let owner = ctx.accounts.owner.key();
        let bump = ctx.accounts.user_account.load()?.bump;
        let seeds = &[UserHeader::SEED, store.as_ref(), owner.as_ref(), &[bump]];

        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: claim_vault.to_account_info(),
                    mint: ctx.accounts.token_mint.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: ctx.accounts.user_account.to_account_info(),
                },
            )
            .with_signer(&[seeds]),
            amount,
            ctx.accounts.token_mint.decimals,
        )?;

        EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority).emit_cpi(
            &BuilderFeeClaimed::new(
                &store,
                &ctx.accounts.user_account.key(),
                &ctx.accounts.token_mint.key(),
                amount,
                &ctx.accounts.destination.key(),
            )?,
        )?;

        Ok(())
    }
}
