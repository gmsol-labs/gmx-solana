use std::collections::HashSet;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer_checked, Mint, Token, TokenAccount, TransferChecked},
};
use gmsol_model::utils::apply_factor;
use gmsol_utils::InitSpace;

use crate::{
    constants,
    events::{EventEmitter, GtUpdated, OrderCreated},
    ops::{
        execution_fee::TransferExecutionFeeOperation,
        order::{CreateOrderOperation, CreateOrderParams},
    },
    order::internal::Close,
    states::{
        common::action::Action,
        feature::ActionDisabledFlag,
        order::{Order, OrderKind},
        position::PositionKind,
        user::UserHeader,
        Market, NonceBytes, Position, RoleKey, Seed, Store, StoreWalletSigner, UpdateOrderParams,
    },
    utils::{internal, token::is_associated_token_account_or_owner},
    CoreError,
};

/// The accounts definition for the [`prepare_position`](crate::gmsol_store::prepare_position)
/// instruction.
#[derive(Accounts)]
#[instruction(params: CreateOrderParams)]
pub struct PreparePosition<'info> {
    /// The owner of the order to be created.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// The position.
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + Position::INIT_SPACE,
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            owner.key().as_ref(),
            market.load()?.meta().market_token_mint.as_ref(),
            params.collateral_token(market.load()?.meta()).as_ref(),
            &[params.to_position_kind()? as u8],
        ],
        bump,
    )]
    pub position: AccountLoader<'info, Position>,
    /// The system program.
    pub system_program: Program<'info, System>,
}

pub(crate) fn prepare_position(
    ctx: Context<PreparePosition>,
    params: &CreateOrderParams,
) -> Result<()> {
    let store = ctx.accounts.store.key();
    let meta = *ctx.accounts.market.load()?.meta();
    let market_token = meta.market_token_mint;
    let collateral_token = params.collateral_token(&meta);
    validate_and_initialize_position_if_needed(
        &ctx.accounts.position,
        ctx.bumps.position,
        params.to_position_kind()?,
        &ctx.accounts.owner,
        collateral_token,
        &market_token,
        &store,
        ctx.accounts.system_program.to_account_info(),
    )?;
    Ok(())
}

fn validate_and_initialize_position_if_needed<'info>(
    position_loader: &AccountLoader<'info, Position>,
    bump: u8,
    kind: PositionKind,
    owner: &AccountInfo<'info>,
    collateral_token: &Pubkey,
    market_token: &Pubkey,
    store: &Pubkey,
    system_program: AccountInfo<'info>,
) -> Result<()> {
    let mut should_transfer_in = false;

    let owner_key = owner.key;
    match position_loader.load_init() {
        Ok(mut position) => {
            position.try_init(
                kind,
                bump,
                *store,
                owner_key,
                market_token,
                collateral_token,
            )?;
            should_transfer_in = true;
            drop(position);
            position_loader.exit(&crate::ID)?;
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
    validate_position(
        &*position_loader.load()?,
        bump,
        kind,
        owner_key,
        collateral_token,
        market_token,
        store,
    )?;

    if should_transfer_in {
        TransferExecutionFeeOperation::builder()
            .payment(position_loader.to_account_info())
            .payer(owner.clone())
            .execution_lamports(Order::position_cut_rent()?)
            .system_program(system_program)
            .build()
            .execute()?;
    }
    Ok(())
}

fn validate_position(
    position: &Position,
    bump: u8,
    kind: PositionKind,
    owner: &Pubkey,
    collateral_token: &Pubkey,
    market_token: &Pubkey,
    store: &Pubkey,
) -> Result<()> {
    require_eq!(position.bump, bump, CoreError::InvalidPosition);
    require_eq!(position.kind()?, kind, CoreError::InvalidPosition);
    require_keys_eq!(position.owner, *owner, CoreError::InvalidPosition);
    require_keys_eq!(
        position.collateral_token,
        *collateral_token,
        CoreError::InvalidPosition
    );
    require_keys_eq!(
        position.market_token,
        *market_token,
        CoreError::InvalidPosition
    );
    require_keys_eq!(position.store, *store, CoreError::InvalidPosition);
    Ok(())
}

/// The accounts definitions for [`create_order`](crate::gmsol_store::create_order) instruction.
///
/// Remaining accounts expected by this instruction:
///
///   - 0..M. `[]` M market accounts, where M represents the length of the
///     swap path for initial collateral token or final output token.
#[derive(Accounts)]
#[instruction(nonce: [u8; 32], params: CreateOrderParams)]
pub struct CreateOrder<'info> {
    /// The owner of the order to be created.
    #[account(mut)]
    pub owner: Signer<'info>,
    /// The receiver of the output funds.
    /// CHECK: only the address is used.
    pub receiver: UncheckedAccount<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
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
    /// The order to be created.
    #[account(
        init,
        space = 8 + Order::INIT_SPACE,
        payer = owner,
        seeds = [Order::SEED, store.key().as_ref(), owner.key().as_ref(), &nonce],
        bump,
    )]
    pub order: AccountLoader<'info, Order>,
    /// The related position.
    #[account(
        mut,
        has_one = store,
        has_one = owner,
        constraint = position.load()?.market_token == market.load()?.meta().market_token_mint @ CoreError::MarketTokenMintMismatched,
        constraint = position.load()?.collateral_token == *params.collateral_token(&*market.load()?) @ CoreError::InvalidPosition,
        constraint = position.load()?.kind()? == params.to_position_kind()? @ CoreError::InvalidPosition,
        seeds = [
            Position::SEED,
            store.key().as_ref(),
            owner.key().as_ref(),
            market.load()?.meta().market_token_mint.as_ref(),
            params.collateral_token(market.load()?.meta()).as_ref(),
            &[params.to_position_kind()? as u8],
        ],
        bump = position.load()?.bump,
    )]
    pub position: Option<AccountLoader<'info, Position>>,
    /// Initial collateral token / swap in token.
    /// Only required by increase and swap orders.
    pub initial_collateral_token: Option<Box<Account<'info, Mint>>>,
    /// Final output token.
    /// Used as collateral token / swap out token for increase and swap orders;
    /// and used as final output token for decrease orders.
    ///
    /// For the case of increase or swap orders, it will be checked to be a valid
    /// collateral / swap out token.
    pub final_output_token: Box<Account<'info, Mint>>,
    /// Long token of the market.
    #[account(constraint = market.load()?.meta().long_token_mint == long_token.key())]
    pub long_token: Option<Box<Account<'info, Mint>>>,
    /// Short token of the market.
    #[account(constraint = market.load()?.meta().short_token_mint == short_token.key())]
    pub short_token: Option<Box<Account<'info, Mint>>>,
    /// Initial collateral token escrow account.
    /// Only requried by increase and swap orders.
    #[account(
        mut,
        associated_token::mint = initial_collateral_token,
        associated_token::authority = order,
    )]
    pub initial_collateral_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// Final output token escrow account.
    /// Only required by decrease and swap orders.
    #[account(
        mut,
        associated_token::mint = final_output_token,
        associated_token::authority = order,
    )]
    pub final_output_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// Long token escrow.
    /// Only required by increase and decrease orders.
    #[account(
        mut,
        associated_token::mint = long_token,
        associated_token::authority = order,
    )]
    pub long_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// Short token escrow.
    /// Only required by increase and decrease orders.
    #[account(
        mut,
        associated_token::mint = short_token,
        associated_token::authority = order,
    )]
    pub short_token_escrow: Option<Box<Account<'info, TokenAccount>>>,
    /// The source initial token account.
    /// Only requried by increase and swap orders.
    #[account(
        mut,
        token::mint = initial_collateral_token,
    )]
    pub initial_collateral_token_source: Option<Box<Account<'info, TokenAccount>>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Create<'info, Order> for CreateOrder<'info> {
    type CreateParams = CreateOrderParams;

    fn action(&self) -> AccountInfo<'info> {
        self.order.to_account_info()
    }

    fn payer(&self) -> AccountInfo<'info> {
        self.owner.to_account_info()
    }

    fn system_program(&self) -> AccountInfo<'info> {
        self.system_program.to_account_info()
    }

    fn validate(&self, params: &Self::CreateParams) -> Result<()> {
        self.store
            .load()?
            .validate_feature_enabled(params.kind.try_into()?, ActionDisabledFlag::CreateOrder)?;
        Ok(())
    }

    fn create_impl(
        &mut self,
        params: &Self::CreateParams,
        nonce: &NonceBytes,
        bumps: &Self::Bumps,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<()> {
        self.transfer_tokens(params)?;

        let ops = CreateOrderOperation::builder()
            .order(self.order.clone())
            .market(self.market.clone())
            .store(self.store.clone())
            .owner(self.owner.to_account_info())
            .receiver(self.receiver.to_account_info())
            .nonce(nonce)
            .bump(bumps.order)
            .params(params)
            .swap_path(remaining_accounts)
            .build();

        let kind = params.kind;
        match kind {
            OrderKind::MarketSwap | OrderKind::LimitSwap => {
                let swap_in = self
                    .initial_collateral_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let swap_out = self
                    .final_output_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                ops.swap()
                    .swap_in_token(swap_in.as_ref())
                    .swap_out_token(swap_out.as_ref())
                    .build()
                    .execute()?;
            }
            OrderKind::MarketIncrease | OrderKind::LimitIncrease => {
                let initial_collateral = self
                    .initial_collateral_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let long_token = self
                    .long_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let short_token = self
                    .short_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                ops.increase()
                    .position(
                        self.position
                            .as_ref()
                            .ok_or_else(|| error!(CoreError::PositionIsRequired))?,
                    )
                    .initial_collateral_token(initial_collateral.as_ref())
                    .long_token(long_token.as_ref())
                    .short_token(short_token.as_ref())
                    .build()
                    .execute()?;
            }
            OrderKind::MarketDecrease | OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let final_output = self
                    .final_output_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let long_token = self
                    .long_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let short_token = self
                    .short_token_escrow
                    .as_ref()
                    .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                ops.decrease()
                    .position(
                        self.position
                            .as_ref()
                            .ok_or_else(|| error!(CoreError::PositionIsRequired))?,
                    )
                    .final_output_token(final_output.as_ref())
                    .long_token(long_token.as_ref())
                    .short_token(short_token.as_ref())
                    .build()
                    .execute()?;
            }
            _ => {
                return err!(CoreError::OrderKindNotAllowed);
            }
        }
        emit!(OrderCreated::new(
            self.store.key(),
            self.order.key(),
            self.position.as_ref().map(|a| a.key()),
        )?);
        Ok(())
    }
}

impl CreateOrder<'_> {
    fn transfer_tokens(&mut self, params: &CreateOrderParams) -> Result<()> {
        let kind = params.kind;
        if !matches!(
            kind,
            OrderKind::MarketSwap
                | OrderKind::LimitSwap
                | OrderKind::MarketIncrease
                | OrderKind::LimitIncrease
        ) {
            return Ok(());
        }
        let amount = params.initial_collateral_delta_amount;
        if amount != 0 {
            let token = self
                .initial_collateral_token
                .as_ref()
                .ok_or_else(|| error!(CoreError::MissingInitialCollateralToken))?;
            let from = self
                .initial_collateral_token_source
                .as_ref()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
            let to = self
                .initial_collateral_token_escrow
                .as_mut()
                .ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;

            transfer_checked(
                CpiContext::new(
                    self.token_program.to_account_info(),
                    TransferChecked {
                        from: from.to_account_info(),
                        mint: token.to_account_info(),
                        to: to.to_account_info(),
                        authority: self.owner.to_account_info(),
                    },
                ),
                amount,
                token.decimals,
            )?;

            to.reload()?;
        }
        Ok(())
    }
}

/// The accounts definition for the [`close_order`](crate::gmsol_store::close_order) instruction.
#[event_cpi]
#[derive(Accounts)]
pub struct CloseOrder<'info> {
    /// The executor of this instruction.
    pub executor: Signer<'info>,
    /// The store.
    #[account(mut)]
    pub store: AccountLoader<'info, Store>,
    /// The store wallet.
    #[account(mut, seeds = [Store::WALLET_SEED, store.key().as_ref()], bump)]
    pub store_wallet: SystemAccount<'info>,
    /// The owner of the order.
    /// CHECK: only used to validate and receive input funds.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,
    /// The receiver of the order.
    /// CHECK: only used to validate and receive output funds.
    #[account(mut)]
    pub receiver: UncheckedAccount<'info>,
    /// The rent receiver of the order.
    /// CHECK: only used to validate and receive rent.
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
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
    /// Referrer User Account.
    #[account(
        mut,
        constraint = referrer_user.key() != user.key() @ CoreError::InvalidArgument,
        constraint = referrer_user.load()?.is_initialized() @ CoreError::InvalidUserAccount,
        constraint = referrer_user.load()?.owner == *user.load()?.referral().referrer().ok_or(CoreError::InvalidArgument)? @ CoreError::InvalidArgument,
        has_one = store,
        seeds = [UserHeader::SEED, store.key().as_ref(), user.load()?.referral().referrer().ok_or(CoreError::InvalidArgument)?.as_ref()],
        bump = referrer_user.load()?.bump,
    )]
    pub referrer_user: Option<AccountLoader<'info, UserHeader>>,
    /// Order to close.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.owner == owner.key() @ CoreError::OwnerMismatched,
        constraint = order.load()?.header.receiver() == receiver.key() @ CoreError::ReceiverMismatched,
        constraint = order.load()?.header.rent_receiver() == rent_receiver.key @ CoreError::RentReceiverMismatched,
        constraint = order.load()?.tokens.initial_collateral.account() == initial_collateral_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.final_output_token.account() == final_output_token_escrow.as_ref().map(|a| a.key()) @ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.long_token.account() == long_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
        constraint = order.load()?.tokens.short_token.account() == short_token_escrow.as_ref().map(|a| a.key())@ CoreError::TokenAccountMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
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
    /// The ATA for initial collateral token of the owner.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(initial_collateral_token_ata.key, owner.key, &initial_collateral_token.as_ref().map(|a| a.key()).expect("must provide")) @ CoreError::NotAnATA,
    )]
    pub initial_collateral_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for final output token of the receiver.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(final_output_token_ata.key, receiver.key, &final_output_token.as_ref().map(|a| a.key()).expect("must provide")) @ CoreError::NotAnATA,
    )]
    pub final_output_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for long token of the receiver.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(long_token_ata.key, receiver.key, &long_token.as_ref().map(|a| a.key()).expect("must provide")) @ CoreError::NotAnATA,
    )]
    pub long_token_ata: Option<UncheckedAccount<'info>>,
    /// The ATA for initial collateral token of the receiver.
    /// CHECK: should be checked during the execution.
    #[account(
        mut,
        constraint = is_associated_token_account_or_owner(short_token_ata.key, receiver.key, &short_token.as_ref().map(|a| a.key()).expect("must provide")) @ CoreError::NotAnATA,
    )]
    pub short_token_ata: Option<UncheckedAccount<'info>>,
    /// The system program.
    pub system_program: Program<'info, System>,
    /// The token program.
    pub token_program: Program<'info, Token>,
    /// The associated token program.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> internal::Authentication<'info> for CloseOrder<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.executor
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}

impl<'info> internal::Close<'info, Order> for CloseOrder<'info> {
    fn expected_keeper_role(&self) -> &str {
        RoleKey::ORDER_KEEPER
    }

    fn rent_receiver(&self) -> AccountInfo<'info> {
        self.rent_receiver.to_account_info()
    }

    fn validate(&self) -> Result<()> {
        let order = self.order.load()?;
        if order.header.action_state()?.is_pending() {
            self.store.load()?.validate_feature_enabled(
                order.params().kind()?.try_into()?,
                ActionDisabledFlag::CancelOrder,
            )?;
        }
        Ok(())
    }

    fn store_wallet_bump(&self, bumps: &Self::Bumps) -> u8 {
        bumps.store_wallet
    }

    fn process(
        &self,
        init_if_needed: bool,
        store_wallet_signer: &StoreWalletSigner,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<internal::Success> {
        let transfer_success = self.transfer_to_atas(init_if_needed, store_wallet_signer)?;
        let process_success = self.process_gt_reward(event_emitter)?;
        Ok(transfer_success && process_success)
    }

    fn event_authority(&self, bumps: &Self::Bumps) -> (AccountInfo<'info>, u8) {
        (
            self.event_authority.to_account_info(),
            bumps.event_authority,
        )
    }

    fn action(&self) -> &AccountLoader<'info, Order> {
        &self.order
    }
}

impl<'info> CloseOrder<'info> {
    fn transfer_to_atas(
        &self,
        init_if_needed: bool,
        store_wallet_signer: &StoreWalletSigner,
    ) -> Result<internal::Success> {
        use crate::utils::token::TransferAllFromEscrowToATA;

        let signer = self.order.load()?.signer();
        let seeds = signer.as_seeds();

        let mut seen = HashSet::<_>::default();

        let builder = TransferAllFromEscrowToATA::builder()
            .store_wallet(self.store_wallet.to_account_info())
            .store_wallet_signer(store_wallet_signer)
            .system_program(self.system_program.to_account_info())
            .token_program(self.token_program.to_account_info())
            .associated_token_program(self.associated_token_program.to_account_info())
            .payer(self.executor.to_account_info())
            .escrow_authority(self.order.to_account_info())
            .escrow_authority_seeds(&seeds)
            .rent_receiver(self.rent_receiver())
            .init_if_needed(init_if_needed)
            .should_unwrap_native(self.order.load()?.header().should_unwrap_native_token());

        let state = self.order.load()?.header().action_state()?;

        // If the order is not completed, transfer input funds to the owner before transferring output funds.
        if !state.is_completed() {
            let (escrow, ata, token) = (
                self.initial_collateral_token_escrow.as_ref(),
                self.initial_collateral_token_ata.as_ref(),
                self.initial_collateral_token.as_ref(),
            );

            if let Some(escrow) = escrow {
                seen.insert(escrow.key());

                let ata = ata.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let token = token.ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;

                if !builder
                    .clone()
                    .mint(token.to_account_info())
                    .decimals(token.decimals)
                    .ata(ata.to_account_info())
                    .escrow(escrow.to_account_info())
                    .owner(self.owner.to_account_info())
                    .build()
                    .unchecked_execute()?
                {
                    return Ok(false);
                }
            }
        }

        // Transfer output funds to the owner.
        for (escrow, ata, token) in [
            (
                self.final_output_token_escrow.as_ref(),
                self.final_output_token_ata.as_ref(),
                self.final_output_token.as_ref(),
            ),
            (
                self.long_token_escrow.as_ref(),
                self.long_token_ata.as_ref(),
                self.long_token.as_ref(),
            ),
            (
                self.short_token_escrow.as_ref(),
                self.short_token_ata.as_ref(),
                self.short_token.as_ref(),
            ),
        ] {
            if let Some(escrow) = escrow {
                if !seen.insert(escrow.key()) {
                    continue;
                }
                let ata = ata.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                let token = token.ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;

                if !builder
                    .clone()
                    .mint(token.to_account_info())
                    .decimals(token.decimals)
                    .ata(ata.to_account_info())
                    .escrow(escrow.to_account_info())
                    .owner(self.receiver.to_account_info())
                    .build()
                    .unchecked_execute()?
                {
                    return Ok(false);
                }
            }
        }

        // If the order is completed, transfer input funds to the owner after transferring output funds.
        if state.is_completed() {
            let (escrow, ata, token) = (
                self.initial_collateral_token_escrow.as_ref(),
                self.initial_collateral_token_ata.as_ref(),
                self.initial_collateral_token.as_ref(),
            );

            if let Some(escrow) = escrow {
                if !seen.contains(&escrow.key()) {
                    let ata = ata.ok_or_else(|| error!(CoreError::TokenAccountNotProvided))?;
                    let token = token.ok_or_else(|| error!(CoreError::TokenMintNotProvided))?;

                    if !builder
                        .clone()
                        .mint(token.to_account_info())
                        .decimals(token.decimals)
                        .ata(ata.to_account_info())
                        .escrow(escrow.to_account_info())
                        .owner(self.owner.to_account_info())
                        .build()
                        .unchecked_execute()?
                    {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    fn process_gt_reward(
        &self,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<internal::Success> {
        let amount = self.order.load()?.gt_reward;
        if amount != 0 {
            self.mint_gt_reward_for_referrer(amount, event_emitter)?;

            self.order.load_mut()?.gt_reward = 0;
        }

        Ok(true)
    }

    fn mint_gt_reward_for_referrer(
        &self,
        amount: u64,
        event_emitter: &EventEmitter<'_, 'info>,
    ) -> Result<()> {
        // Mint referral reward for the referrer.
        let Some(referrer) = self.user.load()?.referral().referrer().copied() else {
            return Ok(());
        };

        let referrer_user = self
            .referrer_user
            .as_ref()
            .ok_or_else(|| error!(CoreError::InvalidArgument))?;

        require_keys_eq!(
            referrer_user.load()?.owner,
            referrer,
            CoreError::InvalidArgument
        );

        let factor = self
            .store
            .load()?
            .gt()
            .referral_reward_factor(referrer_user.load()?.gt.rank())?;

        let reward: u64 =
            apply_factor::<_, { constants::MARKET_DECIMALS }>(&(amount as u128), &factor)
                .ok_or_else(|| error!(CoreError::InvalidGTConfig))?
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

        if reward != 0 {
            let mut store = self.store.load_mut()?;
            let mut referrer_user = referrer_user.load_mut()?;

            store.gt_mut().mint_to(&mut referrer_user, reward)?;

            event_emitter.emit_cpi(&GtUpdated::rewarded(
                referrer_user.owner,
                amount,
                store.gt(),
            ))?;
        }

        Ok(())
    }
}

/// The accounts definitions for [`update_order`](crate::gmsol_store::update_order).
#[derive(Accounts)]
pub struct UpdateOrder<'info> {
    /// Owner.
    pub owner: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Market.
    #[account(mut, has_one = store)]
    pub market: AccountLoader<'info, Market>,
    /// Order to update.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.header.market == market.key() @ CoreError::MarketMismatched,
        constraint = order.load()?.header.owner== owner.key() @ CoreError::OwnerMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
}

pub(crate) fn update_order(ctx: Context<UpdateOrder>, params: &UpdateOrderParams) -> Result<()> {
    // Validate feature enabled.
    {
        let order = ctx.accounts.order.load()?;
        ctx.accounts.store.load()?.validate_feature_enabled(
            order.params().kind()?.try_into()?,
            ActionDisabledFlag::UpdateOrder,
        )?;
    }

    let id = ctx
        .accounts
        .market
        .load_mut()?
        .indexer_mut()
        .next_order_id()?;
    ctx.accounts.order.load_mut()?.update(id, params)?;
    Ok(())
}

/// The accounts definition for the [`cancel_order_if_no_position`](crate::gmsol_store::cancel_order_if_no_position)
/// instruction.
#[derive(Accounts)]
pub struct CancelOrderIfNoPosition<'info> {
    /// Authority.
    pub authority: Signer<'info>,
    /// Store.
    pub store: AccountLoader<'info, Store>,
    /// Order to check.
    #[account(
        mut,
        constraint = order.load()?.header.store == store.key() @ CoreError::StoreMismatched,
        constraint = order.load()?.params.position().copied() == Some(position.key()) @ CoreError::PositionMismatched,
    )]
    pub order: AccountLoader<'info, Order>,
    /// Validate that the position does not exist (or is owned by the system program).
    pub position: SystemAccount<'info>,
}

/// Cancel order if the position does not exist (or is owned by the system program).
/// # CHECK
/// Only [`ORDER_KEEPER`](crate::states::roles::RoleKey::ORDER_KEEPER) can use.
pub(crate) fn unchecked_cancel_order_if_no_position(
    ctx: Context<CancelOrderIfNoPosition>,
) -> Result<()> {
    // Order must be in the pending state which is checked before the transition.
    ctx.accounts.order.load_mut()?.header.cancelled()
}

impl<'info> internal::Authentication<'info> for CancelOrderIfNoPosition<'info> {
    fn authority(&self) -> &Signer<'info> {
        &self.authority
    }

    fn store(&self) -> &AccountLoader<'info, Store> {
        &self.store
    }
}
