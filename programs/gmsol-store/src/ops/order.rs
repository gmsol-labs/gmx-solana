use anchor_lang::{prelude::*, system_program};
use anchor_spl::token::{Mint, TokenAccount};
use gmsol_model::{
    action::decrease_position::DecreasePositionFlags, num::Unsigned, price::Prices, BaseMarket,
    BaseMarketExt, PnlFactorKind, Position as _, PositionImpactMarketMutExt, PositionMut,
    PositionMutExt, PositionState, PositionStateExt,
};
use typed_builder::TypedBuilder;

use crate::{
    events::TradeData,
    states::{
        common::action::{Action, ActionExt, ActionParams},
        market::{
            revertible::{
                market::RevertibleMarket,
                revertible_position::RevertiblePosition,
                swap_market::{SwapDirection, SwapMarkets},
                Revertible,
            },
            utils::{Adl, ValidateMarketBalances},
        },
        order::{Order, OrderKind, OrderParams, TokenAccounts, TransferOut},
        position::PositionKind,
        user::UserHeader,
        HasMarketMeta, Market, NonceBytes, Oracle, Position, Store, ValidateOracleTime,
    },
    CoreError, ModelError,
};

use super::{execution_fee::TransferExecutionFeeOperation, market::MarketTransferOutOperation};

/// Create Order Arguments.
// #[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CreateOrderParams {
    /// Order Kind.
    pub kind: OrderKind,
    /// Execution fee in lamports.
    pub execution_lamports: u64,
    /// The length of the swap path.
    pub swap_path_length: u8,
    /// Initial collateral / swap in token amount.
    pub initial_collateral_delta_amount: u64,
    /// Size delta value.
    pub size_delta_value: u128,
    /// Is long.
    pub is_long: bool,
    /// Is collateral or the swap out token the long token.
    pub is_collateral_long: bool,
    /// Min output amount or value.
    pub min_output: Option<u128>,
    /// Trigger price.
    pub trigger_price: Option<u128>,
    /// Acceptable price.
    pub acceptable_price: Option<u128>,
}

impl ActionParams for CreateOrderParams {
    fn execution_lamports(&self) -> u64 {
        self.execution_lamports
    }
}

impl CreateOrderParams {
    /// Get the related position kind.
    pub fn to_position_kind(&self) -> Result<PositionKind> {
        if self.kind.is_swap() {
            return err!(CoreError::PositionItNotRequired);
        }
        if self.is_long {
            Ok(PositionKind::Long)
        } else {
            Ok(PositionKind::Short)
        }
    }

    /// Get the collateral token or swap out token address.
    pub fn collateral_token<'a>(&'a self, meta: &'a impl HasMarketMeta) -> &'a Pubkey {
        let meta = meta.market_meta();
        if self.is_collateral_long {
            &meta.long_token_mint
        } else {
            &meta.short_token_mint
        }
    }
}

/// Operations for creating a new order.
#[derive(TypedBuilder)]
pub(crate) struct CreateOrderOperation<'a, 'info> {
    order: AccountLoader<'info, Order>,
    market: AccountLoader<'info, Market>,
    store: AccountLoader<'info, Store>,
    owner: AccountInfo<'info>,
    #[builder(
        default,
        setter(
            strip_option,
            doc = "Set the creator of this order. CHECK: It must be the address derving the order account",
        )
    )]
    creator: Option<AccountInfo<'info>>,
    nonce: &'a NonceBytes,
    bump: u8,
    params: &'a CreateOrderParams,
    swap_path: &'info [AccountInfo<'info>],
}

impl<'a, 'info> CreateOrderOperation<'a, 'info> {
    pub(crate) fn swap(
        self,
    ) -> CreateSwapOrderOperationBuilder<'a, 'info, ((CreateOrderOperation<'a, 'info>,), (), ())>
    {
        CreateSwapOrderOperation::builder().common(self)
    }

    pub(crate) fn increase(
        self,
    ) -> CreateIncreaseOrderOperationBuilder<
        'a,
        'info,
        ((CreateOrderOperation<'a, 'info>,), (), (), (), ()),
    > {
        CreateIncreaseOrderOperation::builder().common(self)
    }

    pub(crate) fn decrease(
        self,
    ) -> CreateDecreaseOrderOperationBuilder<
        'a,
        'info,
        ((CreateOrderOperation<'a, 'info>,), (), (), (), ()),
    > {
        CreateDecreaseOrderOperation::builder().common(self)
    }

    fn validate(&self) -> Result<()> {
        self.market.load()?.validate(&self.store.key())?;
        ActionExt::validate_balance(&self.order, self.params.execution_lamports)?;
        Ok(())
    }

    fn init_with(
        &self,
        f: impl FnOnce(
            &CreateOrderParams,
            &mut TokenAccounts,
            &mut OrderParams,
        ) -> Result<(Pubkey, Pubkey)>,
    ) -> Result<()> {
        let id = self.market.load_mut()?.indexer_mut().next_order_id()?;
        {
            let mut order = self.order.load_init()?;
            let Order {
                header,
                market_token,
                tokens,
                params,
                swap,
                ..
            } = &mut *order;

            header.init(
                id,
                self.store.key(),
                self.market.key(),
                self.owner.key(),
                *self.nonce,
                self.bump,
                self.params.execution_lamports,
            )?;

            if let Some(creator) = self.creator.as_ref() {
                header.unchecked_set_creator(creator.key());
            }

            *market_token = self.market.load()?.meta().market_token_mint;

            let (from, to) = (f)(self.params, tokens, params)?;

            let market = self.market.load()?;
            let meta = market.meta();
            let swap_path = self.swap_path;
            // The secondary path is ignored.
            swap.validate_and_init(
                meta,
                self.params.swap_path_length,
                0,
                swap_path,
                &self.store.key(),
                (&from, &from),
                (&to, &from),
            )?;
        }
        Ok(())
    }
}

/// Operation for creating a new swap order.
#[derive(TypedBuilder)]
pub(crate) struct CreateSwapOrderOperation<'a, 'info> {
    common: CreateOrderOperation<'a, 'info>,
    swap_in_token: &'a Account<'info, TokenAccount>,
    swap_out_token: &'a Account<'info, TokenAccount>,
}

impl<'a, 'info> CreateSwapOrderOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<()> {
        self.common.validate()?;
        self.validate_params_excluding_swap()?;

        self.common.init_with(|create, tokens, params| {
            tokens.initial_collateral.init(self.swap_in_token);
            tokens.final_output_token.init(self.swap_out_token);
            params.init_swap(
                create.kind,
                self.swap_out_token.mint,
                create.initial_collateral_delta_amount,
                create.min_output,
            )?;
            Ok((self.swap_in_token.mint, self.swap_out_token.mint))
        })?;
        Ok(())
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        require!(self.common.params.kind.is_swap(), CoreError::Internal);
        require!(
            self.common.params.initial_collateral_delta_amount != 0,
            CoreError::EmptyOrder
        );
        require_gte!(
            self.swap_in_token.amount,
            self.common.params.initial_collateral_delta_amount,
            CoreError::NotEnoughTokenAmount
        );
        require!(
            self.common
                .market
                .load()?
                .meta()
                .is_collateral_token(&self.swap_out_token.mint),
            CoreError::TokenMintMismatched
        );
        Ok(())
    }
}

/// Operation for creating a new increase position order.
#[derive(TypedBuilder)]
pub(crate) struct CreateIncreaseOrderOperation<'a, 'info> {
    common: CreateOrderOperation<'a, 'info>,
    position: &'a AccountLoader<'info, Position>,
    initial_collateral_token: &'a Account<'info, TokenAccount>,
    long_token: &'a Account<'info, TokenAccount>,
    short_token: &'a Account<'info, TokenAccount>,
}

impl<'a, 'info> CreateIncreaseOrderOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<()> {
        self.common.validate()?;
        self.validate_params_excluding_swap()?;

        let collateral_token = if self.common.params.is_collateral_long {
            self.common.market.load()?.meta().long_token_mint
        } else {
            self.common.market.load()?.meta().short_token_mint
        };

        self.common.init_with(|create, tokens, params| {
            tokens
                .initial_collateral
                .init(self.initial_collateral_token);
            tokens.long_token.init(self.long_token);
            tokens.short_token.init(self.short_token);
            params.init_increase(
                create.is_long,
                create.kind,
                self.position.key(),
                collateral_token,
                create.initial_collateral_delta_amount,
                create.size_delta_value,
                create.trigger_price,
                create.acceptable_price,
            )?;
            Ok((self.initial_collateral_token.mint, collateral_token))
        })?;

        Ok(())
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        require!(
            self.common.params.kind.is_increase_position(),
            CoreError::Internal
        );
        require!(
            self.common.params.size_delta_value != 0
                || self.common.params.initial_collateral_delta_amount != 0,
            CoreError::EmptyOrder
        );
        require_gte!(
            self.initial_collateral_token.amount,
            self.common.params.initial_collateral_delta_amount,
            CoreError::NotEnoughTokenAmount
        );

        {
            let market = self.common.market.load()?;
            require_eq!(
                market.meta().long_token_mint,
                self.long_token.mint,
                CoreError::TokenMintMismatched
            );
            require_eq!(
                market.meta().short_token_mint,
                self.short_token.mint,
                CoreError::TokenMintMismatched
            );
            self.position
                .load()?
                .validate_for_market(&market)
                .map_err(ModelError::from)?;
        }

        Ok(())
    }
}

/// Operation for creating a new decrease position order.
#[derive(TypedBuilder)]
pub(crate) struct CreateDecreaseOrderOperation<'a, 'info> {
    common: CreateOrderOperation<'a, 'info>,
    position: &'a AccountLoader<'info, Position>,
    final_output_token: &'a Account<'info, TokenAccount>,
    long_token: &'a Account<'info, TokenAccount>,
    short_token: &'a Account<'info, TokenAccount>,
}

impl<'a, 'info> CreateDecreaseOrderOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<()> {
        self.common.validate()?;
        self.validate_params_excluding_swap()?;

        let collateral_token = if self.common.params.is_collateral_long {
            self.common.market.load()?.meta().long_token_mint
        } else {
            self.common.market.load()?.meta().short_token_mint
        };

        self.common.init_with(|create, tokens, params| {
            tokens.final_output_token.init(self.final_output_token);
            tokens.long_token.init(self.long_token);
            tokens.short_token.init(self.short_token);
            params.init_decrease(
                create.is_long,
                create.kind,
                self.position.key(),
                collateral_token,
                create.initial_collateral_delta_amount,
                create.size_delta_value,
                create.trigger_price,
                create.acceptable_price,
                create.min_output,
            )?;
            Ok((collateral_token, self.final_output_token.mint))
        })?;
        Ok(())
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        require!(
            self.common.params.kind.is_decrease_position(),
            CoreError::Internal
        );
        require!(
            self.common.params.size_delta_value != 0
                || self.common.params.initial_collateral_delta_amount != 0,
            CoreError::EmptyOrder
        );

        {
            let market = self.common.market.load()?;
            require_eq!(
                market.meta().long_token_mint,
                self.long_token.mint,
                CoreError::TokenMintMismatched
            );
            require_eq!(
                market.meta().short_token_mint,
                self.short_token.mint,
                CoreError::TokenMintMismatched
            );
            self.position
                .load()?
                .validate_for_market(&market)
                .map_err(ModelError::from)?;
        }
        Ok(())
    }
}

/// Operation for processing [`TransferOut`].
#[derive(TypedBuilder)]
pub(crate) struct ProcessTransferOutOperation<'a, 'info> {
    token_program: AccountInfo<'info>,
    store: &'a AccountLoader<'info, Store>,
    market: &'a AccountLoader<'info, Market>,
    is_pnl_token_long_token: bool,
    #[builder(default, setter(strip_option))]
    final_output_market: Option<&'a AccountLoader<'info, Market>>,
    final_output_token: Option<&'a Account<'info, Mint>>,
    final_output_token_account: Option<AccountInfo<'info>>,
    final_output_token_vault: Option<&'a Account<'info, TokenAccount>>,
    long_token: Option<&'a Account<'info, Mint>>,
    long_token_account: Option<AccountInfo<'info>>,
    long_token_vault: Option<&'a Account<'info, TokenAccount>>,
    short_token: Option<&'a Account<'info, Mint>>,
    short_token_account: Option<AccountInfo<'info>>,
    short_token_vault: Option<&'a Account<'info, TokenAccount>>,
    pub(crate) claimable_long_token_account_for_user: Option<AccountInfo<'info>>,
    pub(crate) claimable_short_token_account_for_user: Option<AccountInfo<'info>>,
    pub(crate) claimable_pnl_token_account_for_holding: Option<AccountInfo<'info>>,
    transfer_out: &'a TransferOut,
}

impl<'a, 'info> ProcessTransferOutOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<()> {
        let TransferOut {
            final_output_token,
            secondary_output_token,
            long_token,
            short_token,
            long_token_for_claimable_account_of_user,
            short_token_for_claimable_account_of_user,
            long_token_for_claimable_account_of_holding,
            short_token_for_claimable_account_of_holding,
            ..
        } = self.transfer_out;

        if *final_output_token != 0 {
            let (token, market, vault, account) = self.final_output()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .market(market)
                .amount(*final_output_token)
                .to(account.clone())
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .token_program(self.token_program.clone())
                .build()
                .execute()?;
        }

        let (long_token_amount, short_token_amount) = if self.is_pnl_token_long_token {
            (
                secondary_output_token
                    .checked_add(*long_token)
                    .ok_or(error!(CoreError::TokenAmountOverflow))?,
                *short_token,
            )
        } else {
            (
                *long_token,
                secondary_output_token
                    .checked_add(*short_token)
                    .ok_or(error!(CoreError::TokenAmountOverflow))?,
            )
        };

        if long_token_amount != 0 {
            let (token, vault, account) = self.long_token()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(long_token_amount)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }

        if short_token_amount != 0 {
            let (token, vault, account) = self.short_token()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(short_token_amount)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }

        if *long_token_for_claimable_account_of_user != 0 {
            let (token, vault, account) = self.claimable_long_token_account_for_user()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(*long_token_for_claimable_account_of_user)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }

        if *short_token_for_claimable_account_of_user != 0 {
            let (token, vault, account) = self.claimable_short_token_account_for_user()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(*short_token_for_claimable_account_of_user)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }

        if *long_token_for_claimable_account_of_holding != 0 {
            let (token, vault, account) = self.claimable_long_token_account_for_holding()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(*long_token_for_claimable_account_of_holding)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }

        if *short_token_for_claimable_account_of_holding != 0 {
            let (token, vault, account) = self.claimable_short_token_account_for_holding()?;
            MarketTransferOutOperation::builder()
                .store(self.store)
                .token_program(self.token_program.clone())
                .market(self.market)
                .amount(*short_token_for_claimable_account_of_holding)
                .vault(vault.to_account_info())
                .decimals(token.decimals)
                .token_mint(token.to_account_info())
                .to(account.clone())
                .build()
                .execute()?;
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn final_output(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &AccountLoader<'info, Market>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .final_output_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let market = self
            .final_output_market
            .ok_or(error!(CoreError::MarketMismatched))?;
        let vault = self
            .final_output_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .final_output_token_account
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, market, vault, account))
    }

    fn long_token(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .long_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .long_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .long_token_account
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }

    fn short_token(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .short_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .short_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .short_token_account
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }

    fn claimable_long_token_account_for_user(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .long_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .long_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .claimable_long_token_account_for_user
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }

    fn claimable_short_token_account_for_user(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .short_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .short_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .claimable_short_token_account_for_user
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }

    fn claimable_long_token_account_for_holding(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .long_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .long_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .claimable_pnl_token_account_for_holding
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }

    fn claimable_short_token_account_for_holding(
        &self,
    ) -> Result<(
        &Account<'info, Mint>,
        &Account<'info, TokenAccount>,
        &AccountInfo<'info>,
    )> {
        let token = self
            .short_token
            .ok_or(error!(CoreError::TokenMintNotProvided))?;
        let vault = self
            .short_token_vault
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        let account = self
            .claimable_pnl_token_account_for_holding
            .as_ref()
            .ok_or(error!(CoreError::TokenAccountNotProvided))?;
        Ok((token, vault, account))
    }
}

/// Operation for executing order.
#[derive(TypedBuilder)]
pub(crate) struct ExecuteOrderOperation<'a, 'info> {
    executor: AccountInfo<'info>,
    user: &'a AccountLoader<'info, UserHeader>,
    store: &'a AccountLoader<'info, Store>,
    market: &'a AccountLoader<'info, Market>,
    order: &'a AccountLoader<'info, Order>,
    owner: AccountInfo<'info>,
    position: Option<&'a AccountLoader<'info, Position>>,
    event: Option<&'a AccountLoader<'info, TradeData>>,
    oracle: &'a Oracle,
    remaining_accounts: &'info [AccountInfo<'info>],
    throw_on_execution_error: bool,
    #[builder(default)]
    refund: u64,
    system_program: AccountInfo<'info>,
}

pub(crate) type ShouldRemovePosition = bool;
pub(crate) type ShouldSendTradeEvent = bool;

enum SecondaryOrderType {
    Liquidation,
    AutoDeleveraging,
}

impl<'a, 'info> ExecuteOrderOperation<'a, 'info> {
    #[inline(never)]
    pub(crate) fn execute(self) -> Result<(Box<TransferOut>, ShouldSendTradeEvent)> {
        let mut should_close_position = false;

        match self.validate_oracle_and_adl() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired)
                if !self.throw_on_execution_error =>
            {
                msg!(
                    "Order expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok((Box::new(TransferOut::new_failed()), false));
            }
            Err(err) => {
                return Err(error!(err));
            }
        }

        let mut should_throw_error = false;
        let prices = self.market.load()?.prices(self.oracle)?;
        let discount = self.validate_and_get_order_fee_discount()?;
        let res = match self.perform_execution(&mut should_throw_error, prices, discount) {
            Ok((should_remove_position, mut transfer_out, should_send_trade_event)) => {
                transfer_out.set_executed(true);
                should_close_position = should_remove_position;
                Ok((transfer_out, should_send_trade_event))
            }
            Err(err) if !(should_throw_error || self.throw_on_execution_error) => {
                msg!("Execute order error: {}", err);
                should_close_position = self
                    .position
                    .as_ref()
                    .map(|a| Result::Ok(a.load()?.state.is_empty()))
                    .transpose()?
                    .unwrap_or(false);
                Ok((Default::default(), false))
            }
            Err(err) => Err(err),
        };

        let (transfer_out, should_send_trade_event) = res?;

        if should_close_position {
            self.close_position()?;
        }

        Ok((transfer_out, should_send_trade_event))
    }

    #[inline(never)]
    fn validate_and_get_order_fee_discount(&self) -> Result<u128> {
        require!(
            self.user.load()?.is_initialized(),
            CoreError::InvalidUserAccount
        );
        let (rank, is_referred) = {
            let user = self.user.load()?;
            (user.gt.rank(), user.referral.referrer().is_some())
        };
        let discount_factor = self
            .store
            .load()?
            .order_fee_discount_factor(rank, is_referred)?;
        msg!(
            "[Order] apply a {} order fee discount (factor) for this {} rank {} user",
            discount_factor,
            if is_referred {
                "referred"
            } else {
                "non-referred"
            },
            rank,
        );
        Ok(discount_factor)
    }

    #[inline(never)]
    fn perform_execution(
        &self,
        should_throw_error: &mut bool,
        prices: Prices<u128>,
        order_fee_discount_factor: u128,
    ) -> Result<(ShouldRemovePosition, Box<TransferOut>, ShouldSendTradeEvent)> {
        self.validate_market()?;
        self.validate_order(should_throw_error, &prices)?;

        // Prepare execution context.
        let gt_minting_enabled = self.market.load()?.is_gt_minting_enabled();
        let mut market = RevertibleMarket::try_from(self.market)?
            .with_order_fee_discount_factor(order_fee_discount_factor);
        let current_market_token = market.market_meta().market_token_mint;
        let loaders = self
            .order
            .load()?
            .swap
            .unpack_markets_for_swap(&current_market_token, self.remaining_accounts)?;
        let mut swap_markets =
            SwapMarkets::new(&self.store.key(), &loaders, Some(&current_market_token))?;
        let mut transfer_out = Box::default();

        // Distribute position impact.
        {
            let report = market
                .distribute_position_impact()
                .map_err(ModelError::from)?
                .execute()
                .map_err(ModelError::from)?;
            msg!("[Order] pre-execute: {:?}", report);
        }

        let kind = self.order.load()?.params.kind()?;
        let mut should_send_trade_event = false;
        let should_remove_position = match &kind {
            OrderKind::MarketSwap | OrderKind::LimitSwap => {
                execute_swap(
                    should_throw_error,
                    self.oracle,
                    &mut market,
                    &mut swap_markets,
                    &mut transfer_out,
                    &mut *self.order.load_mut()?,
                )?;
                market.commit();
                false
            }
            OrderKind::MarketIncrease
            | OrderKind::MarketDecrease
            | OrderKind::Liquidation
            | OrderKind::AutoDeleveraging
            | OrderKind::LimitIncrease
            | OrderKind::LimitDecrease
            | OrderKind::StopLossDecrease => {
                let position_loader = self
                    .position
                    .as_ref()
                    .ok_or(error!(CoreError::PositionIsRequired))?;
                let event_loader = self
                    .event
                    .as_ref()
                    .ok_or(error!(CoreError::PositionIsRequired))?;
                {
                    let position = position_loader.load()?;
                    let mut event = event_loader.load_mut()?;
                    let is_collateral_long = market
                        .market_meta()
                        .to_token_side(&position.collateral_token)?;
                    event.init(
                        kind.is_increase_position(),
                        is_collateral_long,
                        position_loader.key(),
                        &position,
                        self.order.key(),
                    )?;
                    should_send_trade_event = true;
                }
                let mut position = RevertiblePosition::new(market, position_loader)?;

                position.on_validate().map_err(ModelError::from)?;

                let should_remove_position = match kind {
                    OrderKind::MarketIncrease | OrderKind::LimitIncrease => {
                        execute_increase_position(
                            self.oracle,
                            prices,
                            &mut position,
                            &mut swap_markets,
                            &mut transfer_out,
                            &mut *event_loader.load_mut()?,
                            &mut *self.order.load_mut()?,
                        )?;
                        false
                    }
                    OrderKind::Liquidation => execute_decrease_position(
                        self.oracle,
                        prices,
                        &mut position,
                        &mut swap_markets,
                        &mut transfer_out,
                        &mut *event_loader.load_mut()?,
                        &mut *self.order.load_mut()?,
                        true,
                        Some(SecondaryOrderType::Liquidation),
                    )?,
                    OrderKind::AutoDeleveraging => execute_decrease_position(
                        self.oracle,
                        prices,
                        &mut position,
                        &mut swap_markets,
                        &mut transfer_out,
                        &mut *event_loader.load_mut()?,
                        &mut *self.order.load_mut()?,
                        true,
                        Some(SecondaryOrderType::AutoDeleveraging),
                    )?,
                    OrderKind::MarketDecrease
                    | OrderKind::LimitDecrease
                    | OrderKind::StopLossDecrease => execute_decrease_position(
                        self.oracle,
                        prices,
                        &mut position,
                        &mut swap_markets,
                        &mut transfer_out,
                        &mut *event_loader.load_mut()?,
                        &mut *self.order.load_mut()?,
                        false,
                        None,
                    )?,
                    _ => unreachable!(),
                };

                position.write_to_event(&mut *event_loader.load_mut()?)?;
                event_loader
                    .load_mut()?
                    .update_with_transfer_out(&transfer_out)?;

                if gt_minting_enabled {
                    // FIXME: should we ignore the error to prevent the order being cancelled?
                    self.order.load_mut()?.unchecked_process_gt(
                        &mut *self.store.load_mut()?,
                        &mut *self.user.load_mut()?,
                    )?;
                } else {
                    msg!("[GT] GT minting is disabled for this market");
                }

                position.commit();
                msg!(
                    "[Position] executed with trade_id={}",
                    self.position
                        .as_ref()
                        .unwrap()
                        .load()
                        .unwrap()
                        .state
                        .trade_id
                );
                should_remove_position
            }
        };
        swap_markets.commit();
        Ok((
            should_remove_position,
            transfer_out,
            should_send_trade_event,
        ))
    }

    fn close_position(&self) -> Result<()> {
        let Some(position) = self.position else {
            return err!(CoreError::PositionIsRequired);
        };

        let balance = position.to_account_info().lamports();
        if balance < self.refund {
            msg!(
                "Warn: not enough balance to pay the executor, balance = {}, refund = {}",
                balance,
                self.refund,
            );
        }
        let refund = balance.saturating_sub(self.refund);

        if refund != 0 {
            system_program::transfer(
                CpiContext::new(
                    self.system_program.clone(),
                    system_program::Transfer {
                        from: self.executor.clone(),
                        to: self.owner.clone(),
                    },
                ),
                refund,
            )?;
        }

        position.close(self.executor.clone())?;

        Ok(())
    }

    fn validate_oracle_and_adl(&self) -> crate::CoreResult<()> {
        self.oracle.validate_time(self)?;
        let (kind, is_long) = {
            let order = self.order.load().map_err(|_| CoreError::LoadAccountError)?;
            (
                order
                    .params
                    .kind()
                    .map_err(|_| CoreError::InvalidArgument)?,
                order
                    .params
                    .side()
                    .map_err(|_| CoreError::InvalidArgument)?
                    .is_long(),
            )
        };
        #[allow(clippy::single_match)]
        match kind {
            OrderKind::AutoDeleveraging => {
                self.market
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .validate_adl(self.oracle, is_long)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_market(&self) -> Result<()> {
        self.market.load()?.validate(&self.store.key())?;
        Ok(())
    }

    fn validate_order(&self, should_throw_error: &mut bool, prices: &Prices<u128>) -> Result<()> {
        self.validate_non_empty_order()?;
        match self.validate_trigger_price(prices) {
            Ok(()) => Ok(()),
            Err(err) => {
                if !self.order.load()?.params.kind()?.is_market() {
                    *should_throw_error = true;
                }
                Err(err)
            }
        }
    }

    fn validate_non_empty_order(&self) -> Result<()> {
        let order = self.order.load()?;
        let params = &order.params;
        let kind = params.kind()?;

        // NOTE: we currently allow the delta size for decrease position order to be empty.
        if kind.is_increase_position() {
            require!(params.size_delta_value != 0, CoreError::InvalidArgument);
        }

        if kind.is_swap() {
            require!(
                params.initial_collateral_delta_amount != 0,
                CoreError::InvalidArgument
            );
        }
        Ok(())
    }

    fn validate_trigger_price(&self, prices: &Prices<u128>) -> Result<()> {
        self.order
            .load()?
            .validate_trigger_price(&prices.index_token_price)
    }
}

impl<'a, 'info> ValidateOracleTime for ExecuteOrderOperation<'a, 'info> {
    fn oracle_updated_after(&self) -> crate::CoreResult<Option<i64>> {
        let (kind, updated_at) = {
            let order = self.order.load().map_err(|_| CoreError::LoadAccountError)?;
            (
                order
                    .params
                    .kind()
                    .map_err(|_| CoreError::InvalidArgument)?,
                order.header.updated_at,
            )
        };

        match kind {
            OrderKind::MarketSwap
            | OrderKind::LimitSwap
            | OrderKind::MarketIncrease
            | OrderKind::MarketDecrease
            | OrderKind::LimitIncrease => Ok(Some(updated_at)),
            OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let position = self
                    .position
                    .as_ref()
                    .ok_or(CoreError::PositionIsRequired)?
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?;
                let last_updated = updated_at.max(position.state.increased_at);
                Ok(Some(last_updated))
            }
            OrderKind::Liquidation => {
                let position = self
                    .position
                    .as_ref()
                    .ok_or(CoreError::PositionIsRequired)?
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?;
                Ok(Some(
                    position.state.increased_at.max(position.state.decreased_at),
                ))
            }
            // Ignore the check of oracle ts for ADL orders.
            OrderKind::AutoDeleveraging => Ok(None),
        }
    }

    fn oracle_updated_before(&self) -> crate::CoreResult<Option<i64>> {
        let (kind, updated_at) = {
            let order = self.order.load().map_err(|_| CoreError::LoadAccountError)?;
            (
                order
                    .params
                    .kind()
                    .map_err(|_| CoreError::InvalidArgument)?,
                order.header().updated_at,
            )
        };
        let ts = match kind {
            OrderKind::MarketSwap | OrderKind::MarketIncrease | OrderKind::MarketDecrease => {
                Some(updated_at)
            }
            _ => None,
        };
        ts.map(|ts| {
            self.store
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .request_expiration_at(ts)
        })
        .transpose()
    }

    fn oracle_updated_after_slot(&self) -> crate::CoreResult<Option<u64>> {
        let (kind, updated_at_slot) = {
            let order = self.order.load().map_err(|_| CoreError::LoadAccountError)?;
            (
                order
                    .params
                    .kind()
                    .map_err(|_| CoreError::InvalidArgument)?,
                order.header().updated_at_slot,
            )
        };
        // FIXME: should we validate the slot for liquidation and ADL?
        let after = match kind {
            OrderKind::Liquidation | OrderKind::AutoDeleveraging => None,
            _ => Some(updated_at_slot),
        };
        Ok(after)
    }
}

#[inline(never)]
fn execute_swap(
    should_throw_error: &mut bool,
    oracle: &Oracle,
    market: &mut RevertibleMarket<'_>,
    swap_markets: &mut SwapMarkets<'_>,
    transfer_out: &mut TransferOut,
    order: &mut Order,
) -> Result<()> {
    let swap_out_token = order
        .tokens
        .final_output_token
        .token()
        .ok_or(error!(CoreError::MissingFinalOutputToken))?;
    // Perform swap.
    let swap_out_amount = {
        let swap = &order.swap;
        let initial_collateral_token = order
            .tokens
            .initial_collateral
            .token()
            .ok_or(error!(CoreError::MissingInitialCollateralToken))?;
        let amount = order.params.initial_collateral_delta_amount;
        let (swap_out_amount, _) = swap_markets.revertible_swap(
            SwapDirection::Into(market),
            oracle,
            swap,
            (swap_out_token, swap_out_token),
            (Some(initial_collateral_token), None),
            (amount, 0),
        )?;
        swap_out_amount
    };
    if let Err(err) = order.validate_output_amount(swap_out_amount.into()) {
        if !order.params.kind()?.is_market() {
            *should_throw_error = true;
        }
        return Err(err);
    }
    transfer_out.transfer_out(false, swap_out_amount)?;
    Ok(())
}

#[inline(never)]
fn execute_increase_position(
    oracle: &Oracle,
    prices: Prices<u128>,
    position: &mut RevertiblePosition<'_>,
    swap_markets: &mut SwapMarkets<'_>,
    transfer_out: &mut TransferOut,
    event: &mut TradeData,
    order: &mut Order,
) -> Result<()> {
    let params = &order.params;

    // Perform swap.
    let collateral_increment_amount = {
        let initial_collateral_token = order
            .tokens
            .initial_collateral
            .token()
            .ok_or(error!(CoreError::MissingInitialCollateralToken))?;
        let swap = &order.swap;
        let collateral_token = *position.collateral_token();
        let (collateral_increment_amount, _) = swap_markets.revertible_swap(
            SwapDirection::Into(position.market_mut()),
            oracle,
            swap,
            (collateral_token, collateral_token),
            (Some(initial_collateral_token), None),
            (params.initial_collateral_delta_amount, 0),
        )?;
        collateral_increment_amount
    };

    // Validate that the collateral amount swapped out is sufficient.
    // Here, `min_output` refers to the minimum amount of collateral tokens expected
    // after the swap.
    order.validate_output_amount(collateral_increment_amount.into())?;

    // Increase position.
    let (long_amount, short_amount) = {
        let size_delta_usd = params.size_delta_value;
        let acceptable_price = params.acceptable_price;
        let report = position
            .increase(
                prices,
                collateral_increment_amount.into(),
                size_delta_usd,
                Some(acceptable_price),
            )
            .and_then(|a| a.execute())
            .map_err(ModelError::from)?;
        msg!("[Position] increased: {:?}", report);
        let (long_amount, short_amount) = report.claimable_funding_amounts();
        event.update_with_increase_report(&report)?;
        (*long_amount, *short_amount)
    };

    // Process output amount.
    transfer_out.transfer_out_funding_amounts(&long_amount, &short_amount)?;

    position.market().validate_market_balances(
        long_amount
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        short_amount
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn execute_decrease_position(
    oracle: &Oracle,
    prices: Prices<u128>,
    position: &mut RevertiblePosition<'_>,
    swap_markets: &mut SwapMarkets<'_>,
    transfer_out: &mut TransferOut,
    event: &mut TradeData,
    order: &mut Order,
    is_insolvent_close_allowed: bool,
    secondary_order_type: Option<SecondaryOrderType>,
) -> Result<ShouldRemovePosition> {
    // Decrease position.
    let report = {
        let params = &order.params;
        let collateral_withdrawal_amount = params.initial_collateral_delta_amount as u128;
        let size_delta_usd = params.size_delta_value;
        let acceptable_price = params.acceptable_price;
        let is_liquidation_order =
            matches!(secondary_order_type, Some(SecondaryOrderType::Liquidation));
        let is_adl_order = matches!(
            secondary_order_type,
            Some(SecondaryOrderType::AutoDeleveraging)
        );

        let is_cap_size_delta_usd_allowed = matches!(
            order.params().kind()?,
            OrderKind::LimitDecrease | OrderKind::StopLossDecrease
        );

        // Only required when the order is an ADL order.
        let mut pnl_factor_before_execution = None;

        // Validate the liqudiation is a fully close.
        if is_liquidation_order {
            require_gte!(
                size_delta_usd,
                *position.size_in_usd(),
                CoreError::InvalidArgument
            );
        }

        // Validate that ADL is required.
        if is_adl_order {
            let Some((pnl_factor, _)) = position
                .market()
                .pnl_factor_exceeded(&prices, PnlFactorKind::ForAdl, params.side()?.is_long())
                .map_err(ModelError::from)?
            else {
                return err!(CoreError::AdlNotRequired);
            };
            pnl_factor_before_execution = Some(pnl_factor);
        }

        let report = position
            .decrease(
                prices,
                size_delta_usd,
                Some(acceptable_price),
                collateral_withdrawal_amount,
                DecreasePositionFlags {
                    is_insolvent_close_allowed,
                    is_liquidation_order,
                    is_cap_size_delta_usd_allowed,
                },
            )
            .and_then(|a| a.execute())
            .map_err(ModelError::from)?;

        // Validate that ADL is valid.
        if is_adl_order {
            let pnl_factor_after_execution = position
                .market()
                .pnl_factor(&prices, params.side()?.is_long(), true)
                .map_err(ModelError::from)?;
            require_gt!(
                pnl_factor_before_execution.expect("must be some"),
                pnl_factor_after_execution,
                CoreError::InvalidAdl
            );
            let min_pnl_factor = position
                .market()
                .pnl_factor_config(PnlFactorKind::MinAfterAdl, params.side()?.is_long())
                .and_then(|factor| factor.to_signed())
                .map_err(ModelError::from)?;
            require_gt!(
                pnl_factor_after_execution,
                min_pnl_factor,
                CoreError::InvalidAdl
            );
        }

        msg!("[Position] decreased: {:?}", report);
        event.update_with_decrease_report(&report, &prices)?;
        report
    };
    let should_remove_position = report.should_remove();

    // Perform swaps.
    {
        require!(
            *report.secondary_output_amount() == 0
                || (report.is_output_token_long() != report.is_secondary_output_token_long()),
            CoreError::SameOutputTokensNotMerged,
        );
        let (is_output_token_long, output_amount, secondary_output_amount) = (
            report.is_output_token_long(),
            (*report.output_amount())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
            (*report.secondary_output_amount())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        );

        // Swap output token to the expected output token.
        let meta = *position.market().market_meta();
        let token_ins = if is_output_token_long {
            (Some(meta.long_token_mint), Some(meta.short_token_mint))
        } else {
            (Some(meta.short_token_mint), Some(meta.long_token_mint))
        };

        // Since we have checked that secondary_amount must be zero if output_token == secondary_output_token,
        // the swap should still be correct.

        let final_output_token = order
            .tokens
            .final_output_token
            .token()
            .ok_or(error!(CoreError::MissingFinalOutputToken))?;
        let secondary_output_token = order.secondary_output_token()?;
        let swap = &order.swap;
        let (output_amount, secondary_output_amount) = swap_markets.revertible_swap(
            SwapDirection::From(position.market_mut()),
            oracle,
            swap,
            (final_output_token, secondary_output_token),
            token_ins,
            (output_amount, secondary_output_amount),
        )?;
        order.validate_decrease_output_amounts(
            oracle,
            &final_output_token,
            output_amount,
            &secondary_output_token,
            secondary_output_amount,
        )?;
        transfer_out.transfer_out(false, output_amount)?;
        transfer_out.transfer_out(true, secondary_output_amount)?;
        event.set_final_output_token(&final_output_token);
    }

    // Process other output amounts.
    {
        let (long_amount, short_amount) = report.claimable_funding_amounts();
        transfer_out.transfer_out_funding_amounts(long_amount, short_amount)?;
        transfer_out.process_claimable_collateral_for_decrease(&report)?;
    }

    // Validate market balances.
    let mut long_transfer_out = transfer_out.total_long_token_amount()?;
    let mut short_transfer_out = transfer_out.total_short_token_amount()?;
    let mut add_to_amount = |is_long_token: bool, amount: u64| {
        let acc = if is_long_token {
            &mut long_transfer_out
        } else {
            &mut short_transfer_out
        };
        *acc = acc
            .checked_add(amount)
            .ok_or(error!(CoreError::TokenAmountOverflow))?;
        Result::Ok(())
    };
    let current_market_token = position.market().key();
    let meta = position.market().market_meta();
    let tokens = &order.tokens;
    let output_token_market = order
        .swap
        .last_market_token(true)
        .unwrap_or(&current_market_token);
    let secondary_token_market = order
        .swap
        .last_market_token(false)
        .unwrap_or(&current_market_token);
    if transfer_out.final_output_token != 0 && *output_token_market == current_market_token {
        (add_to_amount)(
            meta.to_token_side(
                tokens
                    .final_output_token
                    .token()
                    .as_ref()
                    .ok_or(error!(CoreError::MissingFinalOutputToken))?,
            )?,
            transfer_out.final_output_token,
        )?;
    }
    if transfer_out.secondary_output_token != 0 && *secondary_token_market == current_market_token {
        (add_to_amount)(
            order.params.side()?.is_long(),
            transfer_out.secondary_output_token,
        )?;
    }
    position
        .market()
        .validate_market_balances(long_transfer_out, short_transfer_out)?;

    Ok(should_remove_position)
}

/// Position Cut Operation.
#[derive(TypedBuilder)]
pub struct PositionCutOperation<'a, 'info> {
    kind: PositionCutKind,
    #[builder(setter(
        doc = "Set the executor of this operation. CHECK: the address of the `order` must be derived from its address"
    ))]
    executor: AccountInfo<'info>,
    position: &'a AccountLoader<'info, Position>,
    event: &'a AccountLoader<'info, TradeData>,
    order: &'a AccountLoader<'info, Order>,
    market: &'a AccountLoader<'info, Market>,
    store: &'a AccountLoader<'info, Store>,
    oracle: &'a Oracle,
    owner: AccountInfo<'info>,
    user: &'a AccountLoader<'info, UserHeader>,
    nonce: &'a NonceBytes,
    order_bump: u8,
    long_token_mint: &'a Account<'info, Mint>,
    short_token_mint: &'a Account<'info, Mint>,
    long_token_account: &'a Account<'info, TokenAccount>,
    short_token_account: &'a Account<'info, TokenAccount>,
    long_token_vault: &'a Account<'info, TokenAccount>,
    short_token_vault: &'a Account<'info, TokenAccount>,
    claimable_long_token_account_for_user: AccountInfo<'info>,
    claimable_short_token_account_for_user: AccountInfo<'info>,
    claimable_pnl_token_account_for_holding: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
    system_program: AccountInfo<'info>,
    refund: u64,
}

/// Position Cut Kind.
#[derive(Clone)]
pub enum PositionCutKind {
    /// Liquidate.
    Liquidate,
    /// AutoDeleverage.
    AutoDeleverage(u128),
}

impl PositionCutKind {
    fn size_delta_usd(&self, size_in_usd: u128) -> u128 {
        match self {
            Self::Liquidate => size_in_usd,
            Self::AutoDeleverage(delta) => size_in_usd.min(*delta),
        }
    }

    fn to_order_kind(&self) -> OrderKind {
        match self {
            Self::Liquidate => OrderKind::Liquidation,
            Self::AutoDeleverage(_) => OrderKind::AutoDeleveraging,
        }
    }
}

impl<'a, 'info> PositionCutOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<ShouldSendTradeEvent> {
        let (size_in_usd, is_long, is_collateral_long) = {
            let position = self.position.load()?;
            let market = self.market.load()?;
            let is_collateral_token_long = market.meta.to_token_side(&position.collateral_token)?;
            (
                position.state.size_in_usd,
                position.try_is_long()?,
                is_collateral_token_long,
            )
        };
        self.create_order(size_in_usd, is_long, is_collateral_long)?;
        let (transfer_out, should_send_trade_event) = self.execute_order()?;
        require!(transfer_out.executed(), CoreError::Internal);
        self.order.load_mut()?.header.completed()?;
        self.process_transfer_out(&transfer_out, is_long, is_collateral_long)?;
        Ok(should_send_trade_event)
    }

    #[inline(never)]
    fn create_order(
        &self,
        size_in_usd: u128,
        is_long: bool,
        is_collateral_long: bool,
    ) -> Result<()> {
        TransferExecutionFeeOperation::builder()
            .payment(self.order.to_account_info())
            .payer(self.executor.to_account_info())
            .execution_lamports(Order::MIN_EXECUTION_LAMPORTS)
            .system_program(self.system_program.to_account_info())
            .build()
            .execute()?;
        let params = CreateOrderParams {
            kind: self.kind.to_order_kind(),
            execution_lamports: Order::MIN_EXECUTION_LAMPORTS,
            swap_path_length: 0,
            initial_collateral_delta_amount: 0,
            size_delta_value: self.kind.size_delta_usd(size_in_usd),
            is_long,
            is_collateral_long,
            min_output: None,
            trigger_price: None,
            acceptable_price: None,
        };
        let output_token_account = if is_collateral_long {
            self.long_token_account
        } else {
            self.short_token_account
        };
        CreateOrderOperation::builder()
            .order(self.order.clone())
            .market(self.market.clone())
            .store(self.store.clone())
            .owner(self.owner.clone())
            .creator(self.executor.clone())
            .nonce(self.nonce)
            .bump(self.order_bump)
            .params(&params)
            .swap_path(&[])
            .build()
            .decrease()
            .position(self.position)
            .final_output_token(output_token_account)
            .long_token(self.long_token_account)
            .short_token(self.short_token_account)
            .build()
            .execute()?;
        // Make sure the discrimator is written to the account data.
        self.order.exit(&crate::ID)?;
        Ok(())
    }

    #[inline(never)]
    fn execute_order(&self) -> Result<(Box<TransferOut>, ShouldSendTradeEvent)> {
        ExecuteOrderOperation::builder()
            .store(self.store)
            .market(self.market)
            .order(self.order)
            .owner(self.owner.clone())
            .user(self.user)
            .position(Some(self.position))
            .event(Some(self.event))
            .oracle(self.oracle)
            .remaining_accounts(&[])
            .throw_on_execution_error(true)
            .refund(self.refund)
            .system_program(self.system_program.clone())
            .executor(self.executor.clone())
            .build()
            .execute()
    }

    #[inline(never)]
    fn process_transfer_out(
        &self,
        transfer_out: &TransferOut,
        is_long: bool,
        is_collateral_long: bool,
    ) -> Result<()> {
        let (output_token, output_token_account, output_token_vault) = if is_collateral_long {
            (
                self.long_token_mint,
                self.long_token_account,
                self.long_token_vault,
            )
        } else {
            (
                self.short_token_mint,
                self.short_token_account,
                self.short_token_vault,
            )
        };
        ProcessTransferOutOperation::builder()
            .token_program(self.token_program.clone())
            .store(self.store)
            .market(self.market)
            .is_pnl_token_long_token(is_long)
            .final_output_market(self.market)
            .final_output_token(Some(output_token))
            .final_output_token_account(Some(output_token_account.to_account_info()))
            .final_output_token_vault(Some(output_token_vault))
            .long_token(Some(self.long_token_mint))
            .long_token_account(Some(self.long_token_account.to_account_info()))
            .long_token_vault(Some(self.long_token_vault))
            .short_token(Some(self.short_token_mint))
            .short_token_account(Some(self.short_token_account.to_account_info()))
            .short_token_vault(Some(self.short_token_vault))
            .claimable_long_token_account_for_user(Some(
                self.claimable_long_token_account_for_user.clone(),
            ))
            .claimable_short_token_account_for_user(Some(
                self.claimable_short_token_account_for_user.clone(),
            ))
            .claimable_pnl_token_account_for_holding(Some(
                self.claimable_pnl_token_account_for_holding.clone(),
            ))
            .transfer_out(transfer_out)
            .build()
            .execute()?;
        Ok(())
    }
}
