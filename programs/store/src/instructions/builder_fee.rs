use anchor_lang::prelude::*;
use anchor_spl::token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked};
use gmsol_model::action::decrease_position::DecreasePositionSwapType;

use crate::{
    events::{BuilderFeeClaimed, BuilderFeeSet, BuilderFeeSettled, EventEmitter},
    states::{
        feature::{ActionDisabledFlag, DomainDisabledFlag},
        user::{UserHeader, USER_TOKEN_CONTROLLER_SEED},
        FactorKey, Order, Seed, Store,
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
            // Upholds the delivery invariant: a settled fee reaches the
            // checkpointed builder and nobody else.
            //
            // A non-zero recorded amount implies a builder must have been set.
            let order = ctx.accounts.order.load()?;
            let builder = order.builder().ok_or_else(|| error!(CoreError::Internal))?;
            require_keys_eq!(*builder, builder_user.key(), CoreError::InvalidUserAccount);
        }

        // `claim_vault` is the ATA of (`builder_user`, `final_output_token`) per
        // the `associated_token` constraint above, so its owner and mint are
        // already guaranteed here.

        // Under the coverage invariant (see the builder fee invariants on
        // `crate::states::order`) the escrow always covers the recorded
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

/// The accounts definitions for
/// [`set_builder_fee`](crate::gmsol_store::set_builder_fee) instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct SetBuilderFee<'info> {
    /// The order's owner, and the only signer that can attach a builder to it.
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// The order to checkpoint the builder fee onto.
    ///
    /// Restricted to orders still pending execution: once an order has been
    /// executed or reached a terminal state its fee is already settled or moot,
    /// and letting a checkpoint land afterwards would change the amount owed
    /// for work already done.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.header.action_state()?.is_pending() @ CoreError::PreconditionsAreNotMet,
    )]
    pub order: AccountLoader<'info, Order>,
    /// The builder's User Account, identifying the builder and advertising the
    /// factor this call must match.
    ///
    /// Not seed-derived here, unlike the owner-scoped User Account elsewhere:
    /// the builder's owner is not a party to this transaction, so there is no
    /// key to derive from. Belonging to this store, and being initialized, are
    /// the whole of what can be checked.
    ///
    /// May be the order owner's own User Account. That is the only way to clear
    /// a checkpoint, since a fresh User Account advertises a zero factor, and it
    /// is deliberately allowed so an owner is never bound to a builder for the
    /// life of the order.
    #[account(
        has_one = store,
        constraint = builder.load()?.is_initialized() @ CoreError::InvalidUserAccount,
    )]
    pub builder: AccountLoader<'info, UserHeader>,
    /// The order's final output token mint, i.e. the token the builder fee will
    /// be denominated in.
    pub final_output_token: Box<Account<'info, Mint>>,
    /// The builder's claim vault: the associated token account of
    /// [`final_output_token`](Self::final_output_token) owned by the builder's
    /// User Account.
    ///
    /// Required to exist here so that settlement cannot fail later. Settlement
    /// takes the vault as a plain token account and creates nothing, so a
    /// builder without one would make the order unsettleable and therefore
    /// uncloseable. Checking at checkpoint time moves that failure to the only
    /// point where it is still recoverable: the owner simply does not get a
    /// builder attached.
    #[account(
        associated_token::mint = final_output_token,
        associated_token::authority = builder,
    )]
    pub claim_vault: Box<Account<'info, TokenAccount>>,
    /// The (per-user, per-token) user token controller PDA.
    ///
    /// Reserved for future withdrawal access control. It has no backing account
    /// and no data today, so the only check performed on it is that the passed
    /// address matches its PDA derivation; adding real behavior behind it later
    /// is not a breaking change to this instruction's accounts.
    /// CHECK: only the PDA derivation is validated, by the seeds below.
    #[account(
        seeds = [
            USER_TOKEN_CONTROLLER_SEED,
            builder.key().as_ref(),
            final_output_token.key().as_ref(),
        ],
        bump,
    )]
    pub user_token_controller: UncheckedAccount<'info>,
    /// Token program.
    ///
    /// Read by the associated-token constraint on
    /// [`claim_vault`](Self::claim_vault) to derive the vault's address.
    pub token_program: Program<'info, Token>,
}

impl SetBuilderFee<'_> {
    pub(crate) fn invoke(ctx: Context<Self>, expected_factor: u128) -> Result<()> {
        // Upholds the liveness invariant: nothing freezes when the mechanism
        // is switched off.
        //
        // The only builder-fee instruction behind the feature flag. Settlement,
        // claiming and execution stay ungated on purpose, so disabling the
        // feature stops new orders from taking on a fee without freezing any
        // fee already owed. `Default` is the action here because the flag guards
        // a mechanism rather than a step in an order's lifecycle.
        ctx.accounts
            .store
            .load()?
            .validate_not_restarted()?
            .validate_feature_enabled(
                DomainDisabledFlag::BuilderFee,
                ActionDisabledFlag::Default,
            )?;

        // Upholds the authorization invariant: never charged at a factor
        // other than the one the owner authorized.
        //
        // The builder's advertised rate has to match what the owner signed for,
        // exactly and with no tolerance: anything looser lets a builder raise
        // its factor between the owner deciding and the transaction landing.
        let factor = ctx.accounts.builder.load()?.builder_fee_factor();
        require_eq!(
            factor,
            expected_factor,
            CoreError::BuilderFeeFactorMismatched
        );

        // Upholds the boundedness invariant: no factor above the cap in force
        // at checkpoint time is ever checkpointed.
        //
        // Second enforcement point for the store cap. The builder's rate was
        // already checked against it when advertised, but the cap can be lowered
        // afterwards, and this is the moment the rate becomes payable.
        //
        // A missing cap is treated as `0` (deny by default), matching how the
        // advertising side reads it.
        let max_factor = ctx
            .accounts
            .store
            .load()?
            .get_factor_by_key(FactorKey::MaxBuilderFeeFactor)
            .copied()
            .unwrap_or(0);
        require_gte!(
            max_factor,
            factor,
            CoreError::BuilderFeeFactorExceedsMaxFactor
        );

        let (previous_builder, previous_factor) = {
            let order = ctx.accounts.order.load()?;
            let kind = order.params().kind()?;

            // Keeper-initiated kinds ride the same decrease pipeline as the
            // owner's own orders, so the check has to name the kinds rather than
            // ask whether the order decreases a position.
            require!(
                kind.is_user_initiated_position(),
                CoreError::BuilderFeeOrderKindNotAllowed
            );

            // `CollateralToPnlToken` moves the entire collateral-token output
            // into the pnl token before the receive-token swap runs, so the
            // final output token bucket a fee is taken from is empty on every
            // such order.
            //
            // The guard is conditional on the factor rather than
            // unconditional, even though rejecting the swap type outright
            // would be harmless today, because a zero-factor checkpoint is the
            // revocation path: it is how an owner clears a builder it no
            // longer wants. Gating on `factor != 0` aims the rule at the harm
            // it exists for, a charge that would be trivially bypassable, and
            // leaves revocation reachable on every order regardless of swap
            // type. The scenario that needs that: should a later change ever
            // let an order's swap type move after a checkpoint is written
            // (`update_order_v2` cannot today), an unconditional guard would
            // strand the existing checkpoint, since the only instruction that
            // could clear it would itself be rejected.
            //
            // Note this runs on every user-initiated position order, not only
            // decrease orders. An increase order leaves the field at
            // `NoSwap`, so the extra breadth is unreachable rather than
            // restrictive.
            if factor != 0
                && matches!(
                    order.params().decrease_position_swap_type()?,
                    DecreasePositionSwapType::CollateralToPnlToken
                )
            {
                return err!(CoreError::BuilderFeeSwapTypeNotAllowed);
            }

            // An increase order created without its final output token escrow
            // has no slot to pay a fee out of. Uninitialized is a real state
            // rather than a sentinel, so `None` is the whole of the check.
            let fee_token = order
                .tokens()
                .final_output_token
                .token()
                .ok_or_else(|| error!(CoreError::BuilderFeeFinalOutputTokenNotInitialized))?;
            require_keys_eq!(
                fee_token,
                ctx.accounts.final_output_token.key(),
                CoreError::TokenMintMismatched
            );

            // An initialized final output token does not by itself guarantee an
            // initialized escrow for it. The two are written together by the
            // creation path, but that is a contract of that path rather than an
            // invariant of the account, so this checks the escrow directly.
            //
            // Without the check, an order that somehow held the token without
            // the escrow could still be checkpointed, and would then revert at
            // the transfer-out stage of `execute_order`, i.e. become
            // unexecutable. That is not a deadlock, since a failed execution
            // charges a zero fee and cancellation still works, but refusing the
            // checkpoint keeps the order usable instead.
            require!(
                order.tokens().final_output_token.account().is_some(),
                CoreError::BuilderFeeFinalOutputTokenEscrowNotInitialized
            );

            (
                order.builder().copied().unwrap_or_default(),
                order.builder_fee_factor(),
            )
        };

        let builder = ctx.accounts.builder.key();
        ctx.accounts
            .order
            .load_mut()?
            .set_builder_fee(builder, factor);

        let event_emitter =
            EventEmitter::new(&ctx.accounts.event_authority, ctx.bumps.event_authority);
        event_emitter.emit_cpi(&BuilderFeeSet::new(
            ctx.accounts.store.key(),
            ctx.accounts.order.key(),
            builder,
            factor,
            previous_builder,
            previous_factor,
        ))?;

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
    /// Must already exist; this instruction never creates it. A builder
    /// with no vault for this mint has never been settled and so has
    /// nothing to claim, which is rejected outright rather than treated
    /// as a no-op: the caller asked to be paid, and the useful answer is
    /// that no such vault exists. Callers holding a `None` vault address
    /// simply leave the instruction out of the transaction.
    #[account(
        mut,
        associated_token::mint = token_mint,
        associated_token::authority = user_account,
    )]
    pub claim_vault: Box<Account<'info, TokenAccount>>,
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
    /// program-controlled User Account PDA. Idempotent: an existing vault
    /// with a zero balance is an explicit no-op, performing no token
    /// transfer, no CPI, and emitting no event, so this is safe to call
    /// freely once the vault exists. Not gated by the `BuilderFee` feature
    /// flag, since disabling the feature must never freeze already-settled
    /// funds.
    pub(crate) fn invoke(ctx: Context<Self>) -> Result<()> {
        let claim_vault = &ctx.accounts.claim_vault;

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
