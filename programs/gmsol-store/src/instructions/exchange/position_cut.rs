use std::ops::Deref;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    check_delegation, constants,
    events::{TradeEvent, TradeEventData},
    get_pnl_token,
    ops::{
        execution_fee::PayExecutionFeeOps,
        order::{PositionCutKind, PositionCutOp},
    },
    states::{
        order::OrderV2, Market, NonceBytes, Oracle, Position, PriceProvider, Seed, Store,
        TokenMapHeader,
    },
    utils::internal,
    validated_recent_timestamp,
};

/// The accounts definitions for the `liquidate` and `auto_deleverage` instructions.
#[event_cpi]
#[derive(Accounts)]
#[instruction(nonce: [u8; 32], recent_timestamp: i64)]
pub struct PositionCut<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The owner of the position.
    /// CHECK: only used to receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// Store.
    #[account(has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Price Provider.
    pub price_provider: Interface<'info, PriceProvider>,
    /// Buffer for oracle prices.
    #[account(mut, has_one = store)]
    pub oracle: Box<Account<'info, Oracle>>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The order to be created.
    #[account(
        init,
        space = 8 + OrderV2::INIT_SPACE,
        payer = authority,
        seeds = [OrderV2::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub order: AccountLoader<'info, OrderV2>,
    #[account(
        mut,
        constraint = position.load()?.owner == owner.key(),
        constraint = position.load()?.store == store.key(),
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            owner.key().as_ref(),
            position.load()?.market_token.as_ref(),
            position.load()?.collateral_token.as_ref(),
            &[position.load()?.kind],
        ],
        bump = position.load()?.bump,
    )]
    pub position: AccountLoader<'info, Position>,
    /// Trade event buffer.
    #[account(mut, has_one = store, has_one = authority)]
    pub event: AccountLoader<'info, TradeEventData>,
    /// Long token.
    pub long_token: Box<Account<'info, Mint>>,
    /// Short token.
    pub short_token: Box<Account<'info, Mint>>,
    /// The escrow account for long tokens.
    #[account(
        mut,
        associated_token::mint = long_token,
        associated_token::authority = order,
    )]
    pub long_token_escrow: Box<Account<'info, TokenAccount>>,
    /// The escrow account for short tokens.
    #[account(
        mut,
        associated_token::mint = short_token,
        associated_token::authority = order,
    )]
    pub short_token_escrow: Box<Account<'info, TokenAccount>>,
    /// Long token vault.
    #[account(
        mut,
        token::mint = long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub long_token_vault: Box<Account<'info, TokenAccount>>,
    /// Short token vault.
    #[account(
        mut,
        token::mint = short_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            short_token_vault.mint.as_ref(),
            &[],
        ],
        bump,
    )]
    pub short_token_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = market.load()?.meta().long_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_long_token_account_for_user, position.load()?.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().long_token_mint.as_ref(),
            position.load()?.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
    pub claimable_long_token_account_for_user: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = market.load()?.meta().short_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_short_token_account_for_user, position.load()?.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().short_token_mint.as_ref(),
            position.load()?.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
    pub claimable_short_token_account_for_user: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = get_pnl_token(&Some(position.clone()), market.load()?.deref())?,
        token::authority = store,
        constraint = check_delegation(&claimable_pnl_token_account_for_holding, store.load()?.address.holding)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            get_pnl_token(&Some(position.clone()), market.load()?.deref())?.as_ref(),
            store.load()?.address.holding.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
    pub claimable_pnl_token_account_for_holding: Box<Account<'info, TokenAccount>>,
    /// Initial collatearl token vault.
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

/// CHECK: only ORDER_KEEPER is allowed to use this instrcution.
pub(crate) fn unchecked_process_position_cut<'info>(
    mut ctx: Context<'_, '_, 'info, 'info, PositionCut<'info>>,
    nonce: &NonceBytes,
    _recent_timestamp: i64,
    kind: PositionCutKind,
    execution_fee: u64,
) -> Result<()> {
    let accounts = &mut ctx.accounts;
    let remaining_accounts = ctx.remaining_accounts;

    let tokens = accounts
        .market
        .load()?
        .meta()
        .ordered_tokens()
        .into_iter()
        .collect::<Vec<_>>();

    let rent = Rent::get()?;
    let refund = rent.minimum_balance(accounts.order.to_account_info().data_len())
        + rent.minimum_balance(accounts.long_token_escrow.to_account_info().data_len())
        + rent.minimum_balance(accounts.short_token_escrow.to_account_info().data_len())
        + OrderV2::MIN_EXECUTION_LAMPORTS;

    let ops = PositionCutOp::builder()
        .kind(kind)
        .position(&accounts.position)
        .order(&accounts.order)
        .event(&accounts.event)
        .market(&accounts.market)
        .store(&accounts.store)
        .owner(accounts.owner.to_account_info())
        .nonce(nonce)
        .order_bump(ctx.bumps.order)
        .long_token_account(&accounts.long_token_escrow)
        .long_token_vault(&accounts.long_token_vault)
        .short_token_account(&accounts.short_token_escrow)
        .short_token_vault(&accounts.short_token_vault)
        .claimable_long_token_account_for_user(
            accounts
                .claimable_long_token_account_for_user
                .to_account_info(),
        )
        .claimable_short_token_account_for_user(
            accounts
                .claimable_short_token_account_for_user
                .to_account_info(),
        )
        .claimable_pnl_token_account_for_holding(
            accounts
                .claimable_pnl_token_account_for_holding
                .to_account_info(),
        )
        .token_program(accounts.token_program.to_account_info())
        .system_program(accounts.system_program.to_account_info())
        .executor(accounts.authority.to_account_info())
        .refund(refund);

    let should_send_trade_event = accounts.oracle.with_prices(
        &accounts.store,
        &accounts.price_provider,
        &accounts.token_map,
        &tokens,
        remaining_accounts,
        |oracle, _remaining_accounts| ops.oracle(oracle).build().execute(),
    )?;

    if should_send_trade_event {
        let event_loader = accounts.event.clone();
        let event = event_loader.load()?;
        let event = TradeEvent::from(&*event);
        event.emit(&accounts.event_authority, ctx.bumps.event_authority)?;
    }

    accounts.pay_execution_fee(execution_fee)?;
    Ok(())
}

impl<'info> internal::Authentication<'info> for PositionCut<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> PositionCut<'info> {
    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = execution_fee.min(self.order.load()?.max_execution_lamports);
        PayExecutionFeeOps::builder()
            .payer(self.order.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }
}
