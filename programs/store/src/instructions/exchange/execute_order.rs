use std::ops::Deref;

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use gmsol_callback::{interface::CallbackInterface, CALLBACK_AUTHORITY_SEED};

use crate::{
    constants,
    events::{EventEmitter, TradeData, TradeEventRef},
    ops::{
        execution_fee::PayExecutionFeeOperation,
        market::{MarketTransferInOperation, MarketTransferOutOperation},
        order::{
            ExecuteOrderOperation, ProcessTransferOutOperation, RemovePosition,
            ShouldSendTradeEvent,
        },
    },
    states::{
        callback::CallbackAuthority,
        common::{
            action::{ActionExt, ActionSigner},
            swap::SwapActionParamsExt,
        },
        feature::ActionDisabledFlag,
        order::{Order, TransferOut},
        position::Position,
        user::UserHeader,
        Chainlink, Market, Oracle, Seed, Store, TokenMapHeader, TokenMapLoader,
    },
    utils::{internal, pubkey::DEFAULT_PUBKEY},
    CoreError,
};

#[allow(deprecated)]
pub use deprecated::*;

/// The accounts definition for [`prepare_trade_event_buffer`](crate::gmsol_store::prepare_trade_event_buffer).
#[derive(Accounts)]
#[instruction(index: u16)]
pub struct PrepareTradeEventBuffer<'info> {
    /// Authority.
    #[account(mut)]
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Trade Event Buffer.
    #[account(
        init_if_needed,
        payer = authority,
        // The "zero-copy" init space of `TradeData` is used here.
        space = 8 + <TradeData as gmsol_utils::InitSpace>::INIT_SPACE,
        seeds = [TradeData::SEED, store.key().as_ref(), authority.key().as_ref(), &index.to_le_bytes()],
        bump,
    )]
    pub event: AccountLoader<'info, TradeData>,
    /// System Program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn prepare_trade_event_buffer(
    ctx: Context<PrepareTradeEventBuffer>,
    _index: u16,
) -> Result<()> {
    match ctx.accounts.event.load_init() {
        Ok(mut event) => {
            require_keys_eq!(event.authority, DEFAULT_PUBKEY, CoreError::Internal);
            event.store = ctx.accounts.store.key();
            event.authority = ctx.accounts.authority.key();
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
    ctx.accounts.event.exit(&crate::ID)?;
    require_keys_eq!(
        ctx.accounts.event.load()?.store,
        ctx.accounts.store.key(),
        CoreError::PermissionDenied
    );
    require_keys_eq!(
        ctx.accounts.event.load()?.authority,
        ctx.accounts.authority.key(),
        CoreError::PermissionDenied
    );
    Ok(())
}

#[inline(never)]
pub(crate) fn get_pnl_token<'a>(
    position: &Option<AccountLoader<'_, Position>>,
    market: &'a Market,
) -> Result<&'a Pubkey> {
    let is_long = position
        .as_ref()
        .ok_or_else(|| error!(CoreError::PositionIsRequired))?
        .load()?
        .try_is_long()?;
    if is_long {
        Ok(&market.meta.long_token_mint)
    } else {
        Ok(&market.meta.short_token_mint)
    }
}

#[inline(never)]
pub(crate) fn check_delegation(account: &TokenAccount, target: &Pubkey) -> Result<bool> {
    let is_matched = account
        .delegate
        .map(|delegate| delegate == *target)
        .ok_or_else(|| error!(CoreError::NoDelegatedAuthorityIsSet))?;
    Ok(is_matched)
}

#[inline(never)]
pub(crate) fn validated_recent_timestamp(config: &Store, timestamp: i64) -> Result<i64> {
    let recent_time_window = config.amount.recent_time_window;
    let expiration_time = timestamp.saturating_add_unsigned(recent_time_window);
    let clock = Clock::get()?;
    if timestamp <= clock.unix_timestamp && clock.unix_timestamp <= expiration_time {
        Ok(timestamp)
    } else {
        err!(CoreError::InvalidArgument)
    }
}

/// The accounts definition for [`execute_increase_or_swap_order_v2`](crate::gmsol_store::execute_increase_or_swap_order_v2) instruction.
///
/// Remaining accounts expected by this instruction:
///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
///     markets excluding the current market in the swap params.
///   - M+N..M+N+V. `[writable]` V virtual inventory accounts, where V represents the total
///     number of unique virtual inventories required by the markets.
#[event_cpi]
#[derive(Accounts)]
#[instruction(recent_timestamp: i64)]
pub struct ExecuteIncreaseOrSwapOrderV2<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut, has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token Map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Oracle buffer to use.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The owner of the order.
    /// CHECK: only used to receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
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
    /// Order to execute.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.params.position().copied() == position.as_ref().map(|p| p.key()) @ CoreError::PositionMismatched,
        constraint = order.load()?.tokens.initial_collateral.account() == initial_collateral_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.final_output_token.account() == final_output_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.long_token.account() == long_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.short_token.account() == short_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
    #[account(
        mut,
        constraint = position.load()?.owner == order.load()?.header.owner,
        constraint = position.load()?.store == store.key(),
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            order.load()?.header.owner.as_ref(),
            position.load()?.market_token.as_ref(),
            position.load()?.collateral_token.as_ref(),
            &[position.load()?.kind],
        ],
        bump = position.load()?.bump,
    )]
    pub position: Option<AccountLoader<'info, Position>>,
    /// Trade event buffer.
    #[account(mut, has_one = store, has_one = authority)]
    pub event: Option<AccountLoader<'info, TradeData>>,
    /// Initial collateral token.
    pub initial_collateral_token: Option<Box<Account<'info, Mint>>>,
    /// Final output token.
    pub final_output_token: Option<Box<Account<'info, Mint>>>,
    /// Long token.
    pub long_token: Option<Box<Account<'info, Mint>>>,
    /// Short token.
    pub short_token: Option<Box<Account<'info, Mint>>>,
    /// The escrow account for initial collateral tokens.
    #[account(
        mut,
        associated_token::mint = initial_collateral_token,
        associated_token::authority = order,
    )]
    pub initial_collateral_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for final output tokens.
    #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
    )]
    pub final_output_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for long tokens.
    #[account(
        mut,
        associated_token::mint = long_token,
        associated_token::authority = order,
    )]
    pub long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The escrow account for short tokens.
    #[account(
        mut,
        associated_token::mint = short_token,
        associated_token::authority = order,
    )]
    pub short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// Initial collateral token vault.
    #[account(
        mut,
        token::mint = initial_collateral_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            initial_collateral_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub initial_collateral_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// Final output token vault.
    #[account(
        mut,
        token::mint = final_output_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_output_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub final_output_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// Long token vault.
    #[account(
        mut,
        token::mint = long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub long_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// Short token vault.
    #[account(
        mut,
        token::mint = short_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            short_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub short_token_vault: Option<Box<Account<'info, TokenAccount>>>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// Callback authority.
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = callback_authority.bump(),
    )]
    pub callback_authority: Option<Account<'info, CallbackAuthority>>,
    /// Callback program.
    pub callback_program: Option<Interface<'info, CallbackInterface>>,
    /// Config account for callback.
    /// CHECK: expected to be checked by the callback program.
    #[account(mut)]
    pub callback_shared_data_account: Option<UncheckedAccount<'info>>,
    /// Action stats account for callback.
    /// CHECK: expected to be checked by the callback program.
    #[account(mut)]
    pub callback_partitioned_data_account: Option<UncheckedAccount<'info>>,
}

impl<'info> internal::Authentication<'info> for ExecuteIncreaseOrSwapOrderV2<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteIncreaseOrSwapOrderV2<'info> {
    #[inline(never)]
    pub(crate) fn invoke(
        mut ctx: Context<'_, '_, 'info, 'info, Self>,
        _recent_timestamp: i64,
        execution_fee: u64,
        throw_on_execution_error: bool,
    ) -> Result<()> {
        let accounts = &mut ctx.accounts;

        let kind = accounts.order.load()?.params().kind()?;

        // Validate the order kind.
        require!(
            kind.is_increase_position() || kind.is_swap(),
            CoreError::InvalidArgument
        );

        // Validate feature enabled.
        accounts.store.load()?.validate_feature_enabled(
            kind.try_into()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
            ActionDisabledFlag::Execute,
        )?;

        let remaining_accounts = ctx.remaining_accounts;
        let signer = accounts.order.load()?.signer();

        let event_authority = accounts.event_authority.clone();
        let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);

        accounts.transfer_tokens_in(&signer, remaining_accounts, &event_emitter)?;

        let (is_position_removed, transfer_out, should_send_trade_event) = accounts
            .perform_execution(remaining_accounts, throw_on_execution_error, &event_emitter)?;

        let executed = transfer_out.executed();
        if executed {
            accounts.order.load_mut()?.header.completed()?;
            accounts.process_transfer_out(remaining_accounts, &transfer_out, &event_emitter)?;
        } else {
            accounts.order.load_mut()?.header.cancelled()?;
            accounts.transfer_tokens_out(remaining_accounts, &event_emitter)?;
        }

        if should_send_trade_event {
            let event_loader = accounts.event.clone();
            let event = event_loader
                .as_ref()
                .ok_or_else(|| error!(CoreError::EventBufferNotProvided))?
                .load()?;
            let event = TradeEventRef::from(&*event);
            event_emitter.emit_cpi(&event)?;
        }

        if is_position_removed {
            msg!("[Position] the position is removed");
        }

        // It must be placed at the end to be executed correctly.
        ctx.accounts.pay_execution_fee(execution_fee)?;

        Ok(())
    }

    #[inline(never)]
    fn transfer_tokens_in(
        &self,
        signer: &ActionSigner,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        if let Some(escrow) = self.initial_collateral_token_escrow.as_ref() {
            let store = &self.store.key();
            let market = self
                .order
                .load()?
                .swap
                .find_and_unpack_first_market(store, true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_collateral_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let amount = self.order.load()?.params.initial_collateral_delta_amount;
            MarketTransferInOperation::builder()
                .store(&self.store)
                .from_authority(self.order.to_account_info())
                .token_program(self.token_program.to_account_info())
                .signer_seeds(&signer.as_seeds())
                .market(&market)
                .from(escrow.to_account_info())
                .vault(vault)
                .amount(amount)
                .event_emitter(*event_emitter)
                .build()
                .execute()?;
        }
        Ok(())
    }

    #[inline(never)]
    fn transfer_tokens_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        if let Some(escrow) = self.initial_collateral_token_escrow.as_ref() {
            let store = &self.store.key();
            let market = self
                .order
                .load()?
                .swap
                .find_and_unpack_first_market(store, true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            let vault = self
                .initial_collateral_token_vault
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let token = self
                .initial_collateral_token
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;
            let amount = self.order.load()?.params.initial_collateral_delta_amount;
            MarketTransferOutOperation::builder()
                .store(&self.store)
                .token_program(self.token_program.to_account_info())
                .market(&market)
                .to(escrow.to_account_info())
                .vault(vault.to_account_info())
                .amount(amount)
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .event_emitter(*event_emitter)
                .build()
                .execute()?;
        }
        Ok(())
    }

    #[inline(never)]
    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<(RemovePosition, Box<TransferOut>, ShouldSendTradeEvent)> {
        // Note: We only need the tokens here, the feeds are not necessary.
        let feeds = self
            .order
            .load()?
            .swap
            .to_feeds(&self.token_map.load_token_map()?)
            .map_err(CoreError::from)?;
        let ops = ExecuteOrderOperation::builder()
            .store(&self.store)
            .market(&self.market)
            .owner(self.owner.to_account_info())
            .user(&self.user)
            .order(&self.order)
            .position(self.position.as_ref())
            .event(self.event.as_ref())
            .throw_on_execution_error(throw_on_execution_error)
            .executor(self.authority.to_account_info())
            .event_emitter(*event_emitter)
            .callback_authority(self.callback_authority.as_ref())
            .callback_program(self.callback_program.as_deref())
            .callback_shared_data_account(self.callback_shared_data_account.as_deref())
            .callback_partitioned_data_account(self.callback_partitioned_data_account.as_deref());

        self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &feeds.tokens,
            remaining_accounts,
            |oracle, remaining_accounts| {
                ops.oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .execute()
            },
        )
    }

    #[inline(never)]
    fn process_transfer_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        transfer_out: &TransferOut,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let is_pnl_token_long_token = self.order.load()?.params.side()?.is_long();
        let final_output_market = self
            .order
            .load()?
            .swap
            .find_and_unpack_last_market(&self.store.key(), true, remaining_accounts)?
            .unwrap_or(self.market.clone());
        ProcessTransferOutOperation::builder()
            .token_program(self.token_program.to_account_info())
            .store(&self.store)
            .market(&self.market)
            .is_pnl_token_long_token(is_pnl_token_long_token)
            .final_output_token(self.final_output_token.as_deref())
            .final_output_market(&final_output_market)
            .final_output_token_account(
                self.final_output_token_escrow
                    .as_ref()
                    .map(|a| a.to_account_info()),
            )
            .final_output_token_vault(self.final_output_token_vault.as_deref())
            .long_token(self.long_token.as_deref())
            .long_token_account(self.long_token_escrow.as_ref().map(|a| a.to_account_info()))
            .long_token_vault(self.long_token_vault.as_deref())
            .short_token(self.short_token.as_deref())
            .short_token_account(
                self.short_token_escrow
                    .as_ref()
                    .map(|a| a.to_account_info()),
            )
            .short_token_vault(self.short_token_vault.as_deref())
            .claimable_long_token_account_for_user(None)
            .claimable_short_token_account_for_user(None)
            .claimable_pnl_token_account_for_holding(None)
            .transfer_out(transfer_out)
            .event_emitter(*event_emitter)
            .build()
            .execute()?;
        Ok(())
    }

    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.order.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.order.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }
}

/// The accounts definition for [`execute_decrease_order_v2`](crate::gmsol_store::execute_decrease_order_v2)
/// instruction.
///
/// Remaining accounts expected by this instruction:
///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
///     swap params.
///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
///     markets excluding the current market in the swap params.
///   - M+N..M+N+V. `[writable]` V virtual inventory accounts, where V represents the total
///     number of unique virtual inventories required by the markets.
#[event_cpi]
#[derive(Accounts)]
#[instruction(recent_timestamp: i64)]
pub struct ExecuteDecreaseOrderV2<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    #[account(mut, has_one = token_map)]
    pub store: AccountLoader<'info, Store>,
    /// Token Map.
    #[account(has_one = store)]
    pub token_map: AccountLoader<'info, TokenMapHeader>,
    /// Oracle buffer to use.
    #[account(mut, has_one = store)]
    pub oracle: AccountLoader<'info, Oracle>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The owner of the order.
    /// CHECK: only used to receive fund.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
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
    /// Order to execute.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.params.position().copied() == Some(position.key()) @ CoreError::PositionMismatched,
        constraint = order.load()?.tokens.final_output_token.account() == Some(final_output_token_escrow.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.long_token.account() == Some(long_token_escrow.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.short_token.account() == Some(short_token_escrow.key()) @ CoreError::TokenAccountMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
    #[account(
        mut,
        constraint = position.load()?.owner == order.load()?.header.owner,
        constraint = position.load()?.store == store.key(),
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            order.load()?.header.owner.as_ref(),
            position.load()?.market_token.as_ref(),
            position.load()?.collateral_token.as_ref(),
            &[position.load()?.kind],
        ],
        bump = position.load()?.bump,
    )]
    pub position: AccountLoader<'info, Position>,
    /// Trade event buffer.
    #[account(mut, has_one = store, has_one = authority)]
    pub event: AccountLoader<'info, TradeData>,
    /// Final output token.
    pub final_output_token: Box<Account<'info, Mint>>,
    /// Long token.
    pub long_token: Box<Account<'info, Mint>>,
    /// Short token.
    pub short_token: Box<Account<'info, Mint>>,
    /// The escrow account for final output tokens.
    #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
    )]
    pub final_output_token_escrow: Box<Account<'info, TokenAccount>>,
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
    /// Final output token vault.
    #[account(
        mut,
        token::mint = final_output_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_output_token_vault.mint.as_ref(),
        ],
        bump,
    )]
    pub final_output_token_vault: Box<Account<'info, TokenAccount>>,
    /// Long token vault.
    #[account(
        mut,
        token::mint = long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_vault.mint.as_ref(),
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
        ],
        bump,
    )]
    pub short_token_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = market.load()?.meta().long_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_long_token_account_for_user, &order.load()?.header.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().long_token_mint.as_ref(),
            order.load()?.header.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
    pub claimable_long_token_account_for_user: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = market.load()?.meta().short_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_short_token_account_for_user, &order.load()?.header.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().short_token_mint.as_ref(),
            order.load()?.header.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
    pub claimable_short_token_account_for_user: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = get_pnl_token(&Some(position.clone()), market.load()?.deref())?,
        token::authority = store,
        constraint = check_delegation(&claimable_pnl_token_account_for_holding, &store.load()?.address.holding)?,
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
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// Callback authority.
    #[account(
        seeds = [CALLBACK_AUTHORITY_SEED],
        bump = callback_authority.bump(),
    )]
    pub callback_authority: Option<Account<'info, CallbackAuthority>>,
    /// Callback program.
    pub callback_program: Option<Interface<'info, CallbackInterface>>,
    /// Config account for callback.
    /// CHECK: expected to be checked by the callback program.
    #[account(mut)]
    pub callback_shared_data_account: Option<UncheckedAccount<'info>>,
    /// Action stats account for callback.
    /// CHECK: expected to be checked by the callback program.
    #[account(mut)]
    pub callback_partitioned_data_account: Option<UncheckedAccount<'info>>,
}

impl<'info> internal::Authentication<'info> for ExecuteDecreaseOrderV2<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> ExecuteDecreaseOrderV2<'info> {
    #[inline(never)]
    pub(crate) fn invoke(
        mut ctx: Context<'_, '_, 'info, 'info, Self>,
        _recent_timestamp: i64,
        execution_fee: u64,
        throw_on_execution_error: bool,
    ) -> Result<()> {
        let accounts = &mut ctx.accounts;
        let remaining_accounts = ctx.remaining_accounts;

        let kind = accounts.order.load()?.params().kind()?;

        // Validate the order kind.
        require!(kind.is_decrease_position(), CoreError::InvalidArgument);

        // Validate feature enabled.
        accounts.store.load()?.validate_feature_enabled(
            kind.try_into()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
            ActionDisabledFlag::Execute,
        )?;

        let event_authority = accounts.event_authority.clone();
        let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);
        let (is_position_removed, transfer_out, should_send_trade_event) = accounts
            .perform_execution(remaining_accounts, throw_on_execution_error, &event_emitter)?;

        let executed = transfer_out.executed();
        if executed {
            accounts.order.load_mut()?.header.completed()?;
            accounts.process_transfer_out(remaining_accounts, &transfer_out, &event_emitter)?;
        } else {
            accounts.order.load_mut()?.header.cancelled()?;
        }

        if should_send_trade_event {
            let event_loader = accounts.event.clone();
            let event = event_loader.load()?;
            let event = TradeEventRef::from(&*event);
            event_emitter.emit_cpi(&event)?;
        }

        if is_position_removed {
            msg!("[Position] the position is removed");
        }

        // It must be placed at the end to be executed correctly.
        ctx.accounts.pay_execution_fee(execution_fee)?;

        Ok(())
    }

    #[inline(never)]
    fn perform_execution(
        &mut self,
        remaining_accounts: &'info [AccountInfo<'info>],
        throw_on_execution_error: bool,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<(RemovePosition, Box<TransferOut>, ShouldSendTradeEvent)> {
        // Note: We only need the tokens here, the feeds are not necessary.
        let feeds = self
            .order
            .load()?
            .swap
            .to_feeds(&self.token_map.load_token_map()?)
            .map_err(CoreError::from)?;
        let ops = ExecuteOrderOperation::builder()
            .store(&self.store)
            .market(&self.market)
            .owner(self.owner.to_account_info())
            .user(&self.user)
            .order(&self.order)
            .position(Some(&self.position))
            .event(Some(&self.event))
            .throw_on_execution_error(throw_on_execution_error)
            .executor(self.authority.to_account_info())
            .event_emitter(*event_emitter)
            .callback_authority(self.callback_authority.as_ref())
            .callback_program(self.callback_program.as_deref())
            .callback_shared_data_account(self.callback_shared_data_account.as_deref())
            .callback_partitioned_data_account(self.callback_partitioned_data_account.as_deref());

        self.oracle.load_mut()?.with_prices(
            &self.store,
            &self.token_map,
            &feeds.tokens,
            remaining_accounts,
            #[inline(never)]
            |oracle, remaining_accounts| {
                ops.oracle(oracle)
                    .remaining_accounts(remaining_accounts)
                    .build()
                    .execute()
            },
        )
    }

    #[inline(never)]
    fn process_transfer_out(
        &self,
        remaining_accounts: &'info [AccountInfo<'info>],
        transfer_out: &TransferOut,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        let is_pnl_token_long_token = self.order.load()?.params.side()?.is_long();
        let final_output_market = self
            .order
            .load()?
            .swap
            .find_and_unpack_last_market(&self.store.key(), true, remaining_accounts)?
            .unwrap_or(self.market.clone());
        ProcessTransferOutOperation::builder()
            .token_program(self.token_program.to_account_info())
            .store(&self.store)
            .market(&self.market)
            .is_pnl_token_long_token(is_pnl_token_long_token)
            .final_output_market(&final_output_market)
            .final_output_token(Some(&self.final_output_token))
            .final_output_token_account(Some(self.final_output_token_escrow.to_account_info()))
            .final_output_token_vault(Some(&*self.final_output_token_vault))
            .long_token(Some(&self.long_token))
            .long_token_account(Some(self.long_token_escrow.to_account_info()))
            .long_token_vault(Some(&*self.long_token_vault))
            .short_token(Some(&self.short_token))
            .short_token_account(Some(self.short_token_escrow.to_account_info()))
            .short_token_vault(Some(&*self.short_token_vault))
            .claimable_long_token_account_for_user(Some(
                self.claimable_long_token_account_for_user.to_account_info(),
            ))
            .claimable_short_token_account_for_user(Some(
                self.claimable_short_token_account_for_user
                    .to_account_info(),
            ))
            .claimable_pnl_token_account_for_holding(Some(
                self.claimable_pnl_token_account_for_holding
                    .to_account_info(),
            ))
            .transfer_out(transfer_out)
            .event_emitter(*event_emitter)
            .build()
            .execute()?;
        Ok(())
    }

    #[inline(never)]
    fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
        let execution_lamports = self.order.load()?.execution_lamports(execution_fee);
        PayExecutionFeeOperation::builder()
            .payer(self.order.to_account_info())
            .receiver(self.authority.to_account_info())
            .execution_lamports(execution_lamports)
            .build()
            .execute()?;
        Ok(())
    }
}

#[deprecated(since = "0.6.0", note = "use v2 instructions instead.")]
mod deprecated {
    use super::*;

    /// The accounts definition for [`execute_increase_or_swap_order`](crate::gmsol_store::execute_increase_or_swap_order) instruction.
    ///
    /// Remaining accounts expected by this instruction:
    ///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
    ///     swap params.
    ///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
    ///     markets excluding the current market in the swap params.
    ///   - M+N..M+N+V. `[writable]` V virtual inventory accounts, where V represents the total
    ///     number of unique virtual inventories required by the markets.
    #[event_cpi]
    #[derive(Accounts)]
    #[instruction(recent_timestamp: i64)]
    pub struct ExecuteIncreaseOrSwapOrder<'info> {
        /// Authority.
        pub authority: Signer<'info>,
        /// Store.
        #[account(mut, has_one = token_map)]
        pub store: AccountLoader<'info, Store>,
        /// Token Map.
        #[account(has_one = store)]
        pub token_map: AccountLoader<'info, TokenMapHeader>,
        /// Oracle buffer to use.
        #[account(mut, has_one = store)]
        pub oracle: AccountLoader<'info, Oracle>,
        /// Market.
        #[account(mut, has_one = store)]
        pub market: AccountLoader<'info, Market>,
        /// The owner of the order.
        /// CHECK: only used to receive fund.
        #[account(mut)]
        pub owner: UncheckedAccount<'info>,
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
        /// Order to execute.
        #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.params.position().copied() == position.as_ref().map(|p| p.key()) @ CoreError::PositionMismatched,
        constraint = order.load()?.tokens.initial_collateral.account() == initial_collateral_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.final_output_token.account() == final_output_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.long_token.account() == long_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.short_token.account() == short_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
    )]
        pub order: AccountLoader<'info, Order>,
        #[account(
        mut,
        constraint = position.load()?.owner == order.load()?.header.owner,
        constraint = position.load()?.store == store.key(),
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            order.load()?.header.owner.as_ref(),
            position.load()?.market_token.as_ref(),
            position.load()?.collateral_token.as_ref(),
            &[position.load()?.kind],
        ],
        bump = position.load()?.bump,
    )]
        pub position: Option<AccountLoader<'info, Position>>,
        /// Trade event buffer.
        #[account(mut, has_one = store, has_one = authority)]
        pub event: Option<AccountLoader<'info, TradeData>>,
        /// Initial collateral token.
        pub initial_collateral_token: Option<Box<Account<'info, Mint>>>,
        /// Final output token.
        pub final_output_token: Option<Box<Account<'info, Mint>>>,
        /// Long token.
        pub long_token: Option<Box<Account<'info, Mint>>>,
        /// Short token.
        pub short_token: Option<Box<Account<'info, Mint>>>,
        /// The escrow account for initial collateral tokens.
        #[account(
        mut,
        associated_token::mint = initial_collateral_token,
        associated_token::authority = order,
    )]
        pub initial_collateral_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
        /// The escrow account for final output tokens.
        #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
    )]
        pub final_output_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
        /// The escrow account for long tokens.
        #[account(
        mut,
        associated_token::mint = long_token,
        associated_token::authority = order,
    )]
        pub long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
        /// The escrow account for short tokens.
        #[account(
        mut,
        associated_token::mint = short_token,
        associated_token::authority = order,
    )]
        pub short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
        /// Initial collateral token vault.
        #[account(
        mut,
        token::mint = initial_collateral_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            initial_collateral_token_vault.mint.as_ref(),
        ],
        bump,
    )]
        pub initial_collateral_token_vault: Option<Box<Account<'info, TokenAccount>>>,
        /// Final output token vault.
        #[account(
        mut,
        token::mint = final_output_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_output_token_vault.mint.as_ref(),
        ],
        bump,
    )]
        pub final_output_token_vault: Option<Box<Account<'info, TokenAccount>>>,
        /// Long token vault.
        #[account(
        mut,
        token::mint = long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_vault.mint.as_ref(),
        ],
        bump,
    )]
        pub long_token_vault: Option<Box<Account<'info, TokenAccount>>>,
        /// Short token vault.
        #[account(
        mut,
        token::mint = short_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            short_token_vault.mint.as_ref(),
        ],
        bump,
    )]
        pub short_token_vault: Option<Box<Account<'info, TokenAccount>>>,
        /// The token program.
        pub token_program: Program<'info, Token>,
        /// The system program.
        pub system_program: Program<'info, System>,
        /// Chainlink Program.
        pub chainlink_program: Option<Program<'info, Chainlink>>,
    }

    #[inline(never)]
    pub(crate) fn unchecked_execute_increase_or_swap_order<'info>(
        mut ctx: Context<'_, '_, 'info, 'info, ExecuteIncreaseOrSwapOrder<'info>>,
        _recent_timestamp: i64,
        execution_fee: u64,
        throw_on_execution_error: bool,
    ) -> Result<()> {
        let accounts = &mut ctx.accounts;

        let kind = accounts.order.load()?.params().kind()?;

        // Validate the order kind.
        require!(
            kind.is_increase_position() || kind.is_swap(),
            CoreError::InvalidArgument
        );

        // Validate feature enabled.
        accounts.store.load()?.validate_feature_enabled(
            kind.try_into()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
            ActionDisabledFlag::Execute,
        )?;

        let remaining_accounts = ctx.remaining_accounts;
        let signer = accounts.order.load()?.signer();

        let event_authority = accounts.event_authority.clone();
        let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);

        accounts.transfer_tokens_in(&signer, remaining_accounts, &event_emitter)?;

        let (is_position_removed, transfer_out, should_send_trade_event) = accounts
            .perform_execution(remaining_accounts, throw_on_execution_error, &event_emitter)?;

        if transfer_out.executed() {
            accounts.order.load_mut()?.header.completed()?;
            accounts.process_transfer_out(remaining_accounts, &transfer_out, &event_emitter)?;
        } else {
            accounts.order.load_mut()?.header.cancelled()?;
            accounts.transfer_tokens_out(remaining_accounts, &event_emitter)?;
        }

        if should_send_trade_event {
            let event_loader = accounts.event.clone();
            let event = event_loader
                .as_ref()
                .ok_or_else(|| error!(CoreError::EventBufferNotProvided))?
                .load()?;
            let event = TradeEventRef::from(&*event);
            event_emitter.emit_cpi(&event)?;
        }

        if is_position_removed {
            msg!("[Position] the position is removed");
        }

        // It must be placed at the end to be executed correctly.
        ctx.accounts.pay_execution_fee(execution_fee)?;

        Ok(())
    }

    impl<'info> internal::Authentication<'info> for ExecuteIncreaseOrSwapOrder<'info> {
        fn authority(&self) -> &Signer<'info> {
            &self.authority
        }

        fn store(&self) -> &AccountLoader<'info, Store> {
            &self.store
        }
    }

    impl<'info> ExecuteIncreaseOrSwapOrder<'info> {
        #[inline(never)]
        fn transfer_tokens_in(
            &self,
            signer: &ActionSigner,
            remaining_accounts: &'info [AccountInfo<'info>],
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<()> {
            if let Some(escrow) = self.initial_collateral_token_escrow.as_ref() {
                let store = &self.store.key();
                let market = self
                    .order
                    .load()?
                    .swap
                    .find_and_unpack_first_market(store, true, remaining_accounts)?
                    .unwrap_or(self.market.clone());
                let vault = self
                    .initial_collateral_token_vault
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let amount = self.order.load()?.params.initial_collateral_delta_amount;
                MarketTransferInOperation::builder()
                    .store(&self.store)
                    .from_authority(self.order.to_account_info())
                    .token_program(self.token_program.to_account_info())
                    .signer_seeds(&signer.as_seeds())
                    .market(&market)
                    .from(escrow.to_account_info())
                    .vault(vault)
                    .amount(amount)
                    .event_emitter(*event_emitter)
                    .build()
                    .execute()?;
            }
            Ok(())
        }

        #[inline(never)]
        fn transfer_tokens_out(
            &self,
            remaining_accounts: &'info [AccountInfo<'info>],
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<()> {
            if let Some(escrow) = self.initial_collateral_token_escrow.as_ref() {
                let store = &self.store.key();
                let market = self
                    .order
                    .load()?
                    .swap
                    .find_and_unpack_first_market(store, true, remaining_accounts)?
                    .unwrap_or(self.market.clone());
                let vault = self
                    .initial_collateral_token_vault
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let token = self
                    .initial_collateral_token
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;
                let amount = self.order.load()?.params.initial_collateral_delta_amount;
                MarketTransferOutOperation::builder()
                    .store(&self.store)
                    .token_program(self.token_program.to_account_info())
                    .market(&market)
                    .to(escrow.to_account_info())
                    .vault(vault.to_account_info())
                    .amount(amount)
                    .decimals(token.decimals)
                    .token_mint(token.to_account_info())
                    .event_emitter(*event_emitter)
                    .build()
                    .execute()?;
            }
            Ok(())
        }

        #[inline(never)]
        fn perform_execution(
            &mut self,
            remaining_accounts: &'info [AccountInfo<'info>],
            throw_on_execution_error: bool,
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<(RemovePosition, Box<TransferOut>, ShouldSendTradeEvent)> {
            // Note: We only need the tokens here, the feeds are not necessary.
            let feeds = self
                .order
                .load()?
                .swap
                .to_feeds(&self.token_map.load_token_map()?)
                .map_err(CoreError::from)?;
            let ops = ExecuteOrderOperation::builder()
                .store(&self.store)
                .market(&self.market)
                .owner(self.owner.to_account_info())
                .user(&self.user)
                .order(&self.order)
                .position(self.position.as_ref())
                .event(self.event.as_ref())
                .throw_on_execution_error(throw_on_execution_error)
                .executor(self.authority.to_account_info())
                .event_emitter(*event_emitter)
                .callback_authority(None)
                .callback_program(None)
                .callback_shared_data_account(None)
                .callback_partitioned_data_account(None);

            self.oracle.load_mut()?.with_prices(
                &self.store,
                &self.token_map,
                &feeds.tokens,
                remaining_accounts,
                |oracle, remaining_accounts| {
                    ops.oracle(oracle)
                        .remaining_accounts(remaining_accounts)
                        .build()
                        .execute()
                },
            )
        }

        #[inline(never)]
        fn process_transfer_out(
            &self,
            remaining_accounts: &'info [AccountInfo<'info>],
            transfer_out: &TransferOut,
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<()> {
            let is_pnl_token_long_token = self.order.load()?.params.side()?.is_long();
            let final_output_market = self
                .order
                .load()?
                .swap
                .find_and_unpack_last_market(&self.store.key(), true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            ProcessTransferOutOperation::builder()
                .token_program(self.token_program.to_account_info())
                .store(&self.store)
                .market(&self.market)
                .is_pnl_token_long_token(is_pnl_token_long_token)
                .final_output_token(self.final_output_token.as_deref())
                .final_output_market(&final_output_market)
                .final_output_token_account(
                    self.final_output_token_escrow
                        .as_ref()
                        .map(|a| a.to_account_info()),
                )
                .final_output_token_vault(self.final_output_token_vault.as_deref())
                .long_token(self.long_token.as_deref())
                .long_token_account(self.long_token_escrow.as_ref().map(|a| a.to_account_info()))
                .long_token_vault(self.long_token_vault.as_deref())
                .short_token(self.short_token.as_deref())
                .short_token_account(
                    self.short_token_escrow
                        .as_ref()
                        .map(|a| a.to_account_info()),
                )
                .short_token_vault(self.short_token_vault.as_deref())
                .claimable_long_token_account_for_user(None)
                .claimable_short_token_account_for_user(None)
                .claimable_pnl_token_account_for_holding(None)
                .transfer_out(transfer_out)
                .event_emitter(*event_emitter)
                .build()
                .execute()?;
            Ok(())
        }

        #[inline(never)]
        fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
            let execution_lamports = self.order.load()?.execution_lamports(execution_fee);
            PayExecutionFeeOperation::builder()
                .payer(self.order.to_account_info())
                .receiver(self.authority.to_account_info())
                .execution_lamports(execution_lamports)
                .build()
                .execute()?;
            Ok(())
        }
    }

    /// The accounts definition for [`execute_decrease_order`](crate::gmsol_store::execute_decrease_order)
    /// instruction.
    ///
    /// Remaining accounts expected by this instruction:
    ///   - 0..M. `[]` M feed accounts, where M represents the total number of tokens in the
    ///     swap params.
    ///   - M..M+N. `[writable]` N market accounts, where N represents the total number of unique
    ///     markets excluding the current market in the swap params.
    ///   - M+N..M+N+V. `[writable]` V virtual inventory accounts, where V represents the total
    ///     number of unique virtual inventories required by the markets.
    #[event_cpi]
    #[derive(Accounts)]
    #[instruction(recent_timestamp: i64)]
    pub struct ExecuteDecreaseOrder<'info> {
        /// Authority.
        pub authority: Signer<'info>,
        /// Store.
        #[account(mut, has_one = token_map)]
        pub store: AccountLoader<'info, Store>,
        /// Token Map.
        #[account(has_one = store)]
        pub token_map: AccountLoader<'info, TokenMapHeader>,
        /// Oracle buffer to use.
        #[account(mut, has_one = store)]
        pub oracle: AccountLoader<'info, Oracle>,
        /// Market.
        #[account(mut, has_one = store)]
        pub market: AccountLoader<'info, Market>,
        /// The owner of the order.
        /// CHECK: only used to receive fund.
        #[account(mut)]
        pub owner: UncheckedAccount<'info>,
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
        /// Order to execute.
        #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.params.position().copied() == Some(position.key()) @ CoreError::PositionMismatched,
        constraint = order.load()?.tokens.final_output_token.account() == Some(final_output_token_escrow.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.long_token.account() == Some(long_token_escrow.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.short_token.account() == Some(short_token_escrow.key()) @ CoreError::TokenAccountMismatched,
    )]
        pub order: AccountLoader<'info, Order>,
        #[account(
        mut,
        constraint = position.load()?.owner == order.load()?.header.owner,
        constraint = position.load()?.store == store.key(),
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            order.load()?.header.owner.as_ref(),
            position.load()?.market_token.as_ref(),
            position.load()?.collateral_token.as_ref(),
            &[position.load()?.kind],
        ],
        bump = position.load()?.bump,
    )]
        pub position: AccountLoader<'info, Position>,
        /// Trade event buffer.
        #[account(mut, has_one = store, has_one = authority)]
        pub event: AccountLoader<'info, TradeData>,
        /// Final output token.
        pub final_output_token: Box<Account<'info, Mint>>,
        /// Long token.
        pub long_token: Box<Account<'info, Mint>>,
        /// Short token.
        pub short_token: Box<Account<'info, Mint>>,
        /// The escrow account for final output tokens.
        #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
    )]
        pub final_output_token_escrow: Box<Account<'info, TokenAccount>>,
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
        /// Final output token vault.
        #[account(
        mut,
        token::mint = final_output_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            final_output_token_vault.mint.as_ref(),
        ],
        bump,
    )]
        pub final_output_token_vault: Box<Account<'info, TokenAccount>>,
        /// Long token vault.
        #[account(
        mut,
        token::mint = long_token,
        token::authority = store,
        seeds = [
            constants::MARKET_VAULT_SEED,
            store.key().as_ref(),
            long_token_vault.mint.as_ref(),
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
        ],
        bump,
    )]
        pub short_token_vault: Box<Account<'info, TokenAccount>>,
        #[account(
        mut,
        token::mint = market.load()?.meta().long_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_long_token_account_for_user, &order.load()?.header.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().long_token_mint.as_ref(),
            order.load()?.header.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
        pub claimable_long_token_account_for_user: Box<Account<'info, TokenAccount>>,
        #[account(
        mut,
        token::mint = market.load()?.meta().short_token_mint,
        token::authority = store,
        constraint = check_delegation(&claimable_short_token_account_for_user, &order.load()?.header.owner)?,
        seeds = [
            constants::CLAIMABLE_ACCOUNT_SEED,
            store.key().as_ref(),
            market.load()?.meta().short_token_mint.as_ref(),
            order.load()?.header.owner.as_ref(),
            &store.load()?.claimable_time_key(validated_recent_timestamp(store.load()?.deref(), recent_timestamp)?)?,
        ],
        bump,
    )]
        pub claimable_short_token_account_for_user: Box<Account<'info, TokenAccount>>,
        #[account(
        mut,
        token::mint = get_pnl_token(&Some(position.clone()), market.load()?.deref())?,
        token::authority = store,
        constraint = check_delegation(&claimable_pnl_token_account_for_holding, &store.load()?.address.holding)?,
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
        /// The token program.
        pub token_program: Program<'info, Token>,
        /// The system program.
        pub system_program: Program<'info, System>,
        /// Chainlink Program.
        pub chainlink_program: Option<Program<'info, Chainlink>>,
    }

    pub(crate) fn unchecked_execute_decrease_order<'info>(
        mut ctx: Context<'_, '_, 'info, 'info, ExecuteDecreaseOrder<'info>>,
        _recent_timestamp: i64,
        execution_fee: u64,
        throw_on_execution_error: bool,
    ) -> Result<()> {
        let accounts = &mut ctx.accounts;
        let remaining_accounts = ctx.remaining_accounts;

        let kind = accounts.order.load()?.params().kind()?;

        // Validate the order kind.
        require!(kind.is_decrease_position(), CoreError::InvalidArgument);

        // Validate feature enabled.
        accounts.store.load()?.validate_feature_enabled(
            kind.try_into()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
            ActionDisabledFlag::Execute,
        )?;

        let event_authority = accounts.event_authority.clone();
        let event_emitter = EventEmitter::new(&event_authority, ctx.bumps.event_authority);
        let (is_position_removed, transfer_out, should_send_trade_event) = accounts
            .perform_execution(remaining_accounts, throw_on_execution_error, &event_emitter)?;

        if transfer_out.executed() {
            accounts.order.load_mut()?.header.completed()?;
            accounts.process_transfer_out(remaining_accounts, &transfer_out, &event_emitter)?;
        } else {
            accounts.order.load_mut()?.header.cancelled()?;
        }

        if should_send_trade_event {
            let event_loader = accounts.event.clone();
            let event = event_loader.load()?;
            let event = TradeEventRef::from(&*event);
            event_emitter.emit_cpi(&event)?;
        }

        if is_position_removed {
            msg!("[Position] the position is removed");
        }

        // It must be placed at the end to be executed correctly.
        ctx.accounts.pay_execution_fee(execution_fee)?;

        Ok(())
    }

    impl<'info> internal::Authentication<'info> for ExecuteDecreaseOrder<'info> {
        fn authority(&self) -> &Signer<'info> {
            &self.authority
        }

        fn store(&self) -> &AccountLoader<'info, Store> {
            &self.store
        }
    }

    impl<'info> ExecuteDecreaseOrder<'info> {
        #[inline(never)]
        fn perform_execution(
            &mut self,
            remaining_accounts: &'info [AccountInfo<'info>],
            throw_on_execution_error: bool,
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<(RemovePosition, Box<TransferOut>, ShouldSendTradeEvent)> {
            // Note: We only need the tokens here, the feeds are not necessary.
            let feeds = self
                .order
                .load()?
                .swap
                .to_feeds(&self.token_map.load_token_map()?)
                .map_err(CoreError::from)?;
            let ops = ExecuteOrderOperation::builder()
                .store(&self.store)
                .market(&self.market)
                .owner(self.owner.to_account_info())
                .user(&self.user)
                .order(&self.order)
                .position(Some(&self.position))
                .event(Some(&self.event))
                .throw_on_execution_error(throw_on_execution_error)
                .executor(self.authority.to_account_info())
                .event_emitter(*event_emitter)
                .callback_authority(None)
                .callback_program(None)
                .callback_shared_data_account(None)
                .callback_partitioned_data_account(None);

            self.oracle.load_mut()?.with_prices(
                &self.store,
                &self.token_map,
                &feeds.tokens,
                remaining_accounts,
                #[inline(never)]
                |oracle, remaining_accounts| {
                    ops.oracle(oracle)
                        .remaining_accounts(remaining_accounts)
                        .build()
                        .execute()
                },
            )
        }

        #[inline(never)]
        fn process_transfer_out(
            &self,
            remaining_accounts: &'info [AccountInfo<'info>],
            transfer_out: &TransferOut,
            event_emitter: &EventEmitter<'_, 'info>,
        ) -> Result<()> {
            let is_pnl_token_long_token = self.order.load()?.params.side()?.is_long();
            let final_output_market = self
                .order
                .load()?
                .swap
                .find_and_unpack_last_market(&self.store.key(), true, remaining_accounts)?
                .unwrap_or(self.market.clone());
            ProcessTransferOutOperation::builder()
                .token_program(self.token_program.to_account_info())
                .store(&self.store)
                .market(&self.market)
                .is_pnl_token_long_token(is_pnl_token_long_token)
                .final_output_market(&final_output_market)
                .final_output_token(Some(&self.final_output_token))
                .final_output_token_account(Some(self.final_output_token_escrow.to_account_info()))
                .final_output_token_vault(Some(&*self.final_output_token_vault))
                .long_token(Some(&self.long_token))
                .long_token_account(Some(self.long_token_escrow.to_account_info()))
                .long_token_vault(Some(&*self.long_token_vault))
                .short_token(Some(&self.short_token))
                .short_token_account(Some(self.short_token_escrow.to_account_info()))
                .short_token_vault(Some(&*self.short_token_vault))
                .claimable_long_token_account_for_user(Some(
                    self.claimable_long_token_account_for_user.to_account_info(),
                ))
                .claimable_short_token_account_for_user(Some(
                    self.claimable_short_token_account_for_user
                        .to_account_info(),
                ))
                .claimable_pnl_token_account_for_holding(Some(
                    self.claimable_pnl_token_account_for_holding
                        .to_account_info(),
                ))
                .transfer_out(transfer_out)
                .event_emitter(*event_emitter)
                .build()
                .execute()?;
            Ok(())
        }

        #[inline(never)]
        fn pay_execution_fee(&self, execution_fee: u64) -> Result<()> {
            let execution_lamports = self.order.load()?.execution_lamports(execution_fee);
            PayExecutionFeeOperation::builder()
                .payer(self.order.to_account_info())
                .receiver(self.authority.to_account_info())
                .execution_lamports(execution_lamports)
                .build()
                .execute()?;
            Ok(())
        }
    }
}
