use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use gmsol_model::{action::decrease_position::DecreasePositionReport, price::Price};
use gmsol_utils::InitSpace as _;

use crate::{events::OrderRemoved, states::FactorKey, CoreError};

use super::{
    common::{
        action::{Action, ActionHeader, ActionSigner, Closable},
        swap::SwapParams,
        token::TokenAndAccount,
    },
    user::UserHeader,
    Oracle, Seed, Store,
};

/// Update Order Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace, Copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct UpdateOrderParams {
    /// Size delta in USD.
    pub size_delta_usd: u128,
    /// Acceptable price.
    pub acceptable_price: Option<u128>,
    /// Trigger price.
    pub trigger_price: Option<u128>,
    /// Min output amount.
    pub min_output_amount: u128,
}

/// Order Kind.
#[derive(
    AnchorSerialize,
    AnchorDeserialize,
    Clone,
    InitSpace,
    Copy,
    strum::EnumString,
    strum::Display,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    Debug,
)]
#[num_enum(error_type(name = CoreError, constructor = CoreError::unknown_order_kind))]
#[strum(serialize_all = "snake_case")]
// #[cfg_attr(feature = "debug", derive(Debug))]
#[non_exhaustive]
#[repr(u8)]
pub enum OrderKind {
    /// Liquidation: allows liquidation of positions if the criteria for liquidation are met.
    Liquidation,
    /// Auto-deleveraging Order.
    AutoDeleveraging,
    /// Swap token A to token B at the current market price.
    ///
    /// The order will be cancelled if the `min_output_amount` cannot be fulfilled.
    MarketSwap,
    /// Increase position at the current market price.
    ///
    /// The order will be cancelled if the position cannot be increased at the acceptable price.
    MarketIncrease,
    /// Decrease position at the current market price.
    ///
    /// The order will be cancelled if the position cannot be decreased at the acceptable price.
    MarketDecrease,
    /// Limit Swap.
    LimitSwap,
    /// Limit Increase.
    LimitIncrease,
    /// Limit Decrease.
    LimitDecrease,
    /// Stop-Loss Decrease.
    StopLossDecrease,
}

impl OrderKind {
    /// Is market order.
    pub fn is_market(&self) -> bool {
        matches!(
            self,
            Self::MarketSwap | Self::MarketIncrease | Self::MarketDecrease
        )
    }

    /// Is swap order.
    pub fn is_swap(&self) -> bool {
        matches!(self, Self::MarketSwap | Self::LimitSwap)
    }

    /// Is increase position order.
    pub fn is_increase_position(&self) -> bool {
        matches!(self, Self::LimitIncrease | Self::MarketIncrease)
    }

    /// Is decrease position order.
    pub fn is_decrease_position(&self) -> bool {
        matches!(
            self,
            Self::LimitDecrease
                | Self::MarketDecrease
                | Self::Liquidation
                | Self::AutoDeleveraging
                | Self::StopLossDecrease
        )
    }
}

/// Transfer Out.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(BorshSerialize, BorshDeserialize, Default)]
pub struct TransferOut {
    /// Executed.
    executed: u8,
    padding_0: [u8; 7],
    /// Final output token.
    pub final_output_token: u64,
    /// Secondary output token.
    pub secondary_output_token: u64,
    /// Long token.
    pub long_token: u64,
    /// Short token.
    pub short_token: u64,
    /// Long token amount for claimable account of user.
    pub long_token_for_claimable_account_of_user: u64,
    /// Short token amount for cliamable account of user.
    pub short_token_for_claimable_account_of_user: u64,
    /// Long token amount for claimable account of holding.
    pub long_token_for_claimable_account_of_holding: u64,
    /// Short token amount for claimable account of holding.
    pub short_token_for_claimable_account_of_holding: u64,
}

/// Recevier Kind.
pub enum CollateralReceiver {
    Collateral,
    ClaimableForHolding,
    ClaimableForUser,
}

impl TransferOut {
    const EXECUTED: u8 = u8::MAX;
    const NOT_EXECUTED: u8 = 0;

    /// Return whether the order is executed.
    pub fn executed(&self) -> bool {
        !self.executed == Self::NOT_EXECUTED
    }

    /// Return whether the output for user is empty.
    pub fn is_user_output_empty(&self) -> bool {
        self.final_output_token == 0
            && self.secondary_output_token == 0
            && self.long_token == 0
            && self.short_token == 0
            && self.long_token_for_claimable_account_of_user == 0
            && self.short_token_for_claimable_account_of_user == 0
    }

    pub(crate) fn set_executed(&mut self, executed: bool) -> &mut Self {
        self.executed = if executed {
            Self::EXECUTED
        } else {
            Self::NOT_EXECUTED
        };
        self
    }

    pub(crate) fn new_failed() -> Self {
        Self {
            executed: Self::NOT_EXECUTED,
            ..Default::default()
        }
    }

    pub(crate) fn total_long_token_amount(&self) -> Result<u64> {
        self.long_token
            .checked_add(self.long_token_for_claimable_account_of_user)
            .and_then(|a| a.checked_add(self.long_token_for_claimable_account_of_holding))
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))
    }

    pub(crate) fn total_short_token_amount(&self) -> Result<u64> {
        self.short_token
            .checked_add(self.short_token_for_claimable_account_of_user)
            .and_then(|a| a.checked_add(self.short_token_for_claimable_account_of_holding))
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))
    }

    pub(crate) fn transfer_out(&mut self, is_secondary: bool, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        if is_secondary {
            self.secondary_output_token = self
                .secondary_output_token
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        } else {
            self.final_output_token = self
                .final_output_token
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        }
        Ok(())
    }

    pub(crate) fn transfer_out_funding_amounts(
        &mut self,
        long_amount: &u128,
        short_amount: &u128,
    ) -> Result<()> {
        self.transfer_out_collateral(
            true,
            CollateralReceiver::Collateral,
            (*long_amount)
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        )?;
        self.transfer_out_collateral(
            false,
            CollateralReceiver::Collateral,
            (*short_amount)
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        )?;
        Ok(())
    }

    pub(crate) fn process_claimable_collateral_for_decrease(
        &mut self,
        report: &DecreasePositionReport<u128>,
    ) -> Result<()> {
        let for_holding = report.claimable_collateral_for_holding();
        require!(
            *for_holding.output_token_amount() == 0,
            CoreError::ClaimableCollateralForHoldingCannotBeInOutputTokens,
        );

        let is_output_token_long = report.is_output_token_long();
        let is_secondary_token_long = report.is_secondary_output_token_long();

        let secondary_amount = (*for_holding.secondary_output_token_amount())
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?;
        self.transfer_out_collateral(
            is_secondary_token_long,
            CollateralReceiver::ClaimableForHolding,
            secondary_amount,
        )?;

        let for_user = report.claimable_collateral_for_user();
        self.transfer_out_collateral(
            is_output_token_long,
            CollateralReceiver::ClaimableForUser,
            (*for_user.output_token_amount())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        )?;
        self.transfer_out_collateral(
            is_secondary_token_long,
            CollateralReceiver::ClaimableForUser,
            (*for_user.secondary_output_token_amount())
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?,
        )?;
        Ok(())
    }

    pub(crate) fn transfer_out_collateral(
        &mut self,
        is_long: bool,
        to: CollateralReceiver,
        amount: u64,
    ) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        match to {
            CollateralReceiver::Collateral => {
                if is_long {
                    self.long_token = self
                        .long_token
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                } else {
                    self.short_token = self
                        .short_token
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                }
            }
            CollateralReceiver::ClaimableForHolding => {
                if is_long {
                    self.long_token_for_claimable_account_of_holding = self
                        .long_token_for_claimable_account_of_holding
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                } else {
                    self.short_token_for_claimable_account_of_holding = self
                        .short_token_for_claimable_account_of_holding
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                }
            }
            CollateralReceiver::ClaimableForUser => {
                if is_long {
                    self.long_token_for_claimable_account_of_user = self
                        .long_token_for_claimable_account_of_user
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                } else {
                    self.short_token_for_claimable_account_of_user = self
                        .short_token_for_claimable_account_of_user
                        .checked_add(amount)
                        .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
                }
            }
        }
        Ok(())
    }
}

/// Order.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Order {
    /// Action header.
    pub(crate) header: ActionHeader,
    /// Market token.
    pub(crate) market_token: Pubkey,
    /// Token accounts.
    pub(crate) tokens: TokenAccounts,
    /// Swap params.
    pub(crate) swap: SwapParams,
    padding_0: [u8; 4],
    /// Order params.
    pub(crate) params: OrderParams,
    pub(crate) gt_reward: u64,
    padding_1: [u8; 8],
    reserve: [u8; 128],
}

impl Seed for Order {
    /// Seed.
    const SEED: &'static [u8] = b"order";
}

impl gmsol_utils::InitSpace for Order {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Action for Order {
    const MIN_EXECUTION_LAMPORTS: u64 = 300_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

impl Closable for Order {
    type ClosedEvent = OrderRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        OrderRemoved::new(
            self.header.id,
            self.header.store,
            *address,
            self.params.kind()?,
            self.market_token,
            self.header.owner,
            self.header.action_state()?,
            reason,
        )
    }
}

impl Order {
    /// Get rent for position cut.
    pub fn position_cut_rent() -> Result<u64> {
        use anchor_spl::token::TokenAccount;

        let rent = Rent::get()?;
        let amount = rent.minimum_balance(Self::INIT_SPACE + 8)
            + rent.minimum_balance(TokenAccount::LEN) * 2
            + Self::MIN_EXECUTION_LAMPORTS;

        Ok(amount)
    }

    /// Get signer.
    pub fn signer(&self) -> ActionSigner {
        self.header.signer(Self::SEED)
    }

    /// Validate trigger price.
    pub fn validate_trigger_price(&self, index_price: &Price<u128>) -> Result<()> {
        let params = &self.params;
        let kind = params.kind()?;
        let is_long = params.side()?.is_long();
        let trigger_price = &params.trigger_price;
        match kind {
            OrderKind::LimitIncrease => {
                if is_long {
                    require_gte!(
                        trigger_price,
                        index_price.pick_price(true),
                        CoreError::InvalidTriggerPrice
                    );
                } else {
                    require_gte!(
                        index_price.pick_price(false),
                        trigger_price,
                        CoreError::InvalidTriggerPrice
                    );
                }
            }
            OrderKind::LimitDecrease => {
                if is_long {
                    require_gte!(
                        index_price.pick_price(false),
                        trigger_price,
                        CoreError::InvalidTriggerPrice
                    );
                } else {
                    require_gte!(
                        trigger_price,
                        index_price.pick_price(true),
                        CoreError::InvalidTriggerPrice
                    );
                }
            }
            OrderKind::StopLossDecrease => {
                if is_long {
                    require_gte!(
                        trigger_price,
                        index_price.pick_price(false),
                        CoreError::InvalidTriggerPrice
                    );
                } else {
                    require_gte!(
                        index_price.pick_price(true),
                        trigger_price,
                        CoreError::InvalidTriggerPrice
                    );
                }
            }
            OrderKind::LimitSwap => {
                // NOTE: For limit swap orders, the trigger price can be substituted by the min output amount,
                // so validatoin is not required. In fact, we should prohibit the creation of limit swap orders
                // with a trigger price.
            }

            OrderKind::MarketSwap
            | OrderKind::MarketIncrease
            | OrderKind::MarketDecrease
            | OrderKind::Liquidation
            | OrderKind::AutoDeleveraging => {}
        }

        Ok(())
    }

    /// Validate output amount.
    pub fn validate_output_amount(&self, output_amount: u128) -> Result<()> {
        require_gte!(
            output_amount,
            self.params.min_output,
            CoreError::InsufficientOutputAmount
        );
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn validate_decrease_output_amounts(
        &self,
        oracle: &Oracle,
        output_token: &Pubkey,
        output_amount: u64,
        secondary_output_token: &Pubkey,
        secondary_output_amount: u64,
    ) -> Result<()> {
        let mut total = 0u128;
        {
            let price = oracle.get_primary_price(output_token)?.min;
            let output_value = u128::from(output_amount).saturating_mul(price);
            total = total.saturating_add(output_value);
        }
        {
            let price = oracle.get_primary_price(secondary_output_token)?.min;
            let output_value = u128::from(secondary_output_amount).saturating_mul(price);
            total = total.saturating_add(output_value);
        }

        // We use the `min_output_amount` as min output value.
        self.validate_output_amount(total)?;
        Ok(())
    }

    /// Get secondary output token (pnl token).
    pub fn secondary_output_token(&self) -> Result<Pubkey> {
        if self.params.side()?.is_long() {
            self.tokens.long_token.token()
        } else {
            self.tokens.short_token.token()
        }
        .ok_or_else(|| error!(CoreError::MissingPoolTokens))
    }

    /// Get order params.
    pub fn params(&self) -> &OrderParams {
        &self.params
    }

    /// Get swap params.
    pub fn swap(&self) -> &SwapParams {
        &self.swap
    }

    /// Get market token.
    pub fn market_token(&self) -> &Pubkey {
        &self.market_token
    }

    /// Get token accounts.
    pub fn tokens(&self) -> &TokenAccounts {
        &self.tokens
    }

    /// Process GT.
    /// CHECK: the order must have been successfully executed.
    #[inline(never)]
    pub(crate) fn unchecked_process_gt(
        &mut self,
        store: &mut Store,
        user: &mut UserHeader,
    ) -> Result<()> {
        let size_in_value = self.params.size_delta_value;
        // Ignore the overflowed value.
        let next_traded_value = user.gt.traded_value().saturating_add(size_in_value);
        let minted_value = user.gt.minted_value();

        require_gte!(
            next_traded_value,
            minted_value,
            CoreError::InvalidUserAccount
        );

        let value_to_mint_for = next_traded_value.saturating_sub(minted_value);

        // Apply minting cost discount for referred user.
        let is_referred = user.referral.referrer().is_some();
        let discount = if is_referred {
            *store.get_factor_by_key(FactorKey::GtMintingCostReferredDiscount)
        } else {
            0
        };

        let (minted, delta_minted_value) = store
            .gt_mut()
            .get_mint_amount(value_to_mint_for, discount)?;

        let next_minted_value = minted_value
            .checked_add(delta_minted_value)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        store.gt_mut().mint_to(user, minted)?;
        msg!("[GT] minted {} units of GT", minted);

        self.gt_reward = minted;
        user.gt.traded_value = next_traded_value;
        user.gt.minted_value = next_minted_value;

        Ok(())
    }

    pub(crate) fn update(&mut self, id: u64, params: &UpdateOrderParams) -> Result<()> {
        let current = &mut self.params;
        require!(current.is_updatable()?, CoreError::InvalidArgument);

        self.header.id = id;
        current.size_delta_value = params.size_delta_usd;
        if let Some(acceptable_price) = params.acceptable_price {
            current.acceptable_price = acceptable_price;
        }
        if let Some(trigger_price) = params.trigger_price {
            current.trigger_price = trigger_price;
        }
        current.min_output = params.min_output_amount;

        self.header.updated()?;

        Ok(())
    }
}

/// Token accounts for Order.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenAccounts {
    /// Initial collateral.
    pub(crate) initial_collateral: TokenAndAccount,
    /// Final output token.
    pub(crate) final_output_token: TokenAndAccount,
    /// Long token.
    pub(crate) long_token: TokenAndAccount,
    /// Short token.
    pub(crate) short_token: TokenAndAccount,
}

impl TokenAccounts {
    /// Get initial collateral token info.
    ///
    /// Only available for increase and swap orders.
    pub fn initial_collateral(&self) -> &TokenAndAccount {
        &self.initial_collateral
    }

    /// Get final output token info.
    ///
    /// Only available for decrease and swap orders.
    pub fn final_output_token(&self) -> &TokenAndAccount {
        &self.final_output_token
    }

    /// Get long token info.
    ///
    /// Only available for increase and decrease orders.
    pub fn long_token(&self) -> &TokenAndAccount {
        &self.long_token
    }

    /// Get short token info.
    ///
    //// Only available for increase and decrease orders.
    pub fn short_token(&self) -> &TokenAndAccount {
        &self.short_token
    }
}

/// Order params.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct OrderParams {
    /// Kind.
    kind: u8,
    /// Order side.
    side: u8,
    padding_1: [u8; 6],
    /// Collateral/Output token.
    collateral_token: Pubkey,
    /// Position address.
    position: Pubkey,
    /// Initial collateral delta amount.
    pub(crate) initial_collateral_delta_amount: u64,
    /// Size delta value.
    pub(crate) size_delta_value: u128,
    /// Min output amount or value.
    /// - Used as amount for swap orders.
    /// - Used as value for decrease position orders.
    min_output: u128,
    /// Trigger price (in unit price).
    pub(crate) trigger_price: u128,
    /// Acceptable price (in unit price).
    pub(crate) acceptable_price: u128,
    reserve: [u8; 64],
}

impl OrderParams {
    pub(crate) fn init_swap(
        &mut self,
        kind: OrderKind,
        collateral_token: Pubkey,
        swap_in_amount: u64,
        min_output: Option<u128>,
    ) -> Result<()> {
        self.kind = kind.into();
        self.collateral_token = collateral_token;
        self.initial_collateral_delta_amount = swap_in_amount;
        match kind {
            OrderKind::MarketSwap => {
                self.min_output = min_output.unwrap_or(0);
            }
            OrderKind::LimitSwap => {
                let Some(min_output) = min_output else {
                    return err!(CoreError::InvalidMinOutputAmount);
                };
                require!(min_output != 0, CoreError::Internal);
                self.min_output = min_output;
            }
            _ => {
                return err!(CoreError::Internal);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init_increase(
        &mut self,
        is_long: bool,
        kind: OrderKind,
        position: Pubkey,
        collateral_token: Pubkey,
        initial_collateral_delta_amount: u64,
        size_delta_value: u128,
        trigger_price: Option<u128>,
        acceptable_price: Option<u128>,
    ) -> Result<()> {
        self.kind = kind.into();
        self.side = if is_long {
            OrderSide::Long
        } else {
            OrderSide::Short
        }
        .into();
        self.collateral_token = collateral_token;
        self.initial_collateral_delta_amount = initial_collateral_delta_amount;
        self.size_delta_value = size_delta_value;
        self.position = position;
        match acceptable_price {
            Some(price) => {
                self.acceptable_price = price;
            }
            None => {
                if is_long {
                    self.acceptable_price = u128::MAX;
                } else {
                    self.acceptable_price = u128::MIN;
                }
            }
        }
        match kind {
            OrderKind::MarketIncrease => {
                require!(trigger_price.is_none(), CoreError::InvalidTriggerPrice);
            }
            OrderKind::LimitIncrease => {
                let Some(price) = trigger_price else {
                    return err!(CoreError::InvalidTriggerPrice);
                };
                self.trigger_price = price;
            }
            _ => {
                return err!(CoreError::Internal);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init_decrease(
        &mut self,
        is_long: bool,
        kind: OrderKind,
        position: Pubkey,
        collateral_token: Pubkey,
        initial_collateral_delta_amount: u64,
        size_delta_value: u128,
        trigger_price: Option<u128>,
        acceptable_price: Option<u128>,
        min_output: Option<u128>,
    ) -> Result<()> {
        self.kind = kind.into();
        self.side = if is_long {
            OrderSide::Long
        } else {
            OrderSide::Short
        }
        .into();
        self.position = position;
        self.collateral_token = collateral_token;
        self.initial_collateral_delta_amount = initial_collateral_delta_amount;
        self.size_delta_value = size_delta_value;
        self.min_output = min_output.unwrap_or(0);
        match acceptable_price {
            Some(price) => {
                self.acceptable_price = price;
            }
            None => {
                if is_long {
                    self.acceptable_price = u128::MIN;
                } else {
                    self.acceptable_price = u128::MAX;
                }
            }
        }
        match kind {
            OrderKind::MarketDecrease | OrderKind::Liquidation | OrderKind::AutoDeleveraging => {
                require!(trigger_price.is_none(), CoreError::InvalidTriggerPrice);
            }
            OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let Some(price) = trigger_price else {
                    return err!(CoreError::InvalidTriggerPrice);
                };
                self.trigger_price = price;
            }
            _ => {
                return err!(CoreError::Internal);
            }
        }
        Ok(())
    }

    /// Get order kind.
    pub fn kind(&self) -> Result<OrderKind> {
        Ok(self.kind.try_into()?)
    }

    /// Return whether the order is updatable.
    pub fn is_updatable(&self) -> Result<bool> {
        let kind = self.kind()?;
        Ok(matches!(
            kind,
            OrderKind::LimitSwap
                | OrderKind::LimitIncrease
                | OrderKind::LimitDecrease
                | OrderKind::StopLossDecrease
        ))
    }

    /// Get order side.
    pub fn side(&self) -> Result<OrderSide> {
        let side = self.side.try_into()?;
        Ok(side)
    }

    /// Get position address.
    pub fn position(&self) -> Option<&Pubkey> {
        if self.position != Pubkey::default() {
            Some(&self.position)
        } else {
            None
        }
    }

    /// Get initial collateral delta amount.
    pub fn amount(&self) -> u64 {
        self.initial_collateral_delta_amount
    }

    /// Get size delta in value.
    pub fn size(&self) -> u128 {
        self.size_delta_value
    }

    /// Get accetable price (unit price).
    pub fn acceptable_price(&self) -> u128 {
        self.acceptable_price
    }

    /// Get trigger price (unit price).
    pub fn trigger_price(&self) -> u128 {
        self.trigger_price
    }

    /// Get min output.
    pub fn min_output(&self) -> u128 {
        self.min_output
    }
}

/// Order side.
#[derive(
    Clone,
    Copy,
    strum::EnumString,
    strum::Display,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
#[num_enum(error_type(name = CoreError, constructor = CoreError::unknown_order_side))]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "debug", derive(Debug))]
#[non_exhaustive]
#[repr(u8)]
pub enum OrderSide {
    /// Long.
    Long,
    /// Short.
    Short,
}

impl OrderSide {
    /// Return whether the side is long.
    pub fn is_long(&self) -> bool {
        matches!(self, Self::Long)
    }
}
