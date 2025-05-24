use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use gmsol_model::{
    action::decrease_position::{DecreasePositionReport, DecreasePositionSwapType},
    price::Price,
};
use gmsol_utils::InitSpace as _;

use crate::{
    events::{EventEmitter, GtUpdated, OrderRemoved},
    utils::pubkey::optional_address,
    CoreError,
};

use super::{
    common::{
        action::{Action, ActionHeader, ActionSigner, Closable},
        swap::SwapActionParams,
        token::TokenAndAccount,
    },
    user::UserHeader,
    Oracle, Seed, Store,
};

pub use gmsol_utils::order::{OrderKind, OrderSide};

/// Update Order Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace, Copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct UpdateOrderParams {
    /// Size delta in USD.
    pub size_delta_value: Option<u128>,
    /// Acceptable price.
    pub acceptable_price: Option<u128>,
    /// Trigger price.
    pub trigger_price: Option<u128>,
    /// Min output amount.
    pub min_output: Option<u128>,
    /// Valid from this timestamp.
    pub valid_from_ts: Option<i64>,
}

impl UpdateOrderParams {
    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.size_delta_value.is_none()
            && self.acceptable_price.is_none()
            && self.trigger_price.is_none()
            && self.min_output.is_none()
            && self.valid_from_ts.is_none()
    }
}

/// Transfer Out.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(BorshSerialize, BorshDeserialize, Default, InitSpace)]
pub struct TransferOut {
    /// Executed.
    executed: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
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

#[cfg(test)]
impl From<crate::events::EventTransferOut> for TransferOut {
    fn from(event: crate::events::EventTransferOut) -> Self {
        let crate::events::EventTransferOut {
            executed,
            padding_0,
            final_output_token,
            secondary_output_token,
            long_token,
            short_token,
            long_token_for_claimable_account_of_user,
            short_token_for_claimable_account_of_user,
            long_token_for_claimable_account_of_holding,
            short_token_for_claimable_account_of_holding,
        } = event;

        Self {
            executed,
            padding_0,
            final_output_token,
            secondary_output_token,
            long_token,
            short_token,
            long_token_for_claimable_account_of_user,
            short_token_for_claimable_account_of_user,
            long_token_for_claimable_account_of_holding,
            short_token_for_claimable_account_of_holding,
        }
    }
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
        report: &DecreasePositionReport<u128, i128>,
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
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Order {
    /// Action header.
    pub(crate) header: ActionHeader,
    /// Market token.
    pub(crate) market_token: Pubkey,
    /// Token accounts.
    pub(crate) tokens: OrderTokenAccounts,
    /// Swap params.
    pub(crate) swap: SwapActionParams,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 4],
    /// Order params.
    pub(crate) params: OrderActionParams,
    pub(crate) gt_reward: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 8],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
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
    pub(crate) fn position_cut_rent(is_pure: bool, include_execution_fee: bool) -> Result<u64> {
        use anchor_spl::token::TokenAccount;

        let rent = Rent::get()?;
        let amount = rent.minimum_balance(Self::INIT_SPACE + 8)
            + rent.minimum_balance(TokenAccount::LEN) * if is_pure { 1 } else { 2 }
            + if include_execution_fee {
                Self::MIN_EXECUTION_LAMPORTS
            } else {
                0
            };

        Ok(amount)
    }

    /// Get signer.
    pub fn signer(&self) -> ActionSigner {
        self.header.signer(Self::SEED)
    }

    /// Validate that current timestamp >= `valid_from_ts`.
    pub fn validate_valid_from_ts(&self) -> Result<()> {
        if self.params.kind()?.is_market() {
            return Ok(());
        }
        require_gte!(Clock::get()?.unix_timestamp, self.params.valid_from_ts);
        Ok(())
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
            _ => return err!(CoreError::UnknownOrderKind),
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
            let price = oracle.get_primary_price(output_token, false)?.min;
            let output_value = u128::from(output_amount).saturating_mul(price);
            total = total.saturating_add(output_value);
        }
        {
            let price = oracle.get_primary_price(secondary_output_token, false)?.min;
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
    pub fn params(&self) -> &OrderActionParams {
        &self.params
    }

    /// Get swap params.
    pub fn swap(&self) -> &SwapActionParams {
        &self.swap
    }

    /// Get market token.
    pub fn market_token(&self) -> &Pubkey {
        &self.market_token
    }

    /// Get token accounts.
    pub fn tokens(&self) -> &OrderTokenAccounts {
        &self.tokens
    }

    /// Process GT.
    /// CHECK: the order must have been successfully executed.
    #[inline(never)]
    pub(crate) fn unchecked_process_gt(
        &mut self,
        store: &mut Store,
        user: &mut UserHeader,
        paid_fee_value: u128,
        event_emitter: &EventEmitter,
    ) -> Result<()> {
        if paid_fee_value == 0 {
            msg!("[GT] GT is not minted unless an order fee is paid");
            return Ok(());
        }

        // Ignore the overflowed value.
        let next_paid_fee_value = user.gt.paid_fee_value().saturating_add(paid_fee_value);
        let minted_fee_value = user.gt.minted_fee_value();

        require_gte!(
            next_paid_fee_value,
            minted_fee_value,
            CoreError::InvalidUserAccount
        );

        let value_to_mint_for = next_paid_fee_value.saturating_sub(minted_fee_value);

        let (minted, delta_minted_value, minting_cost) =
            store.gt().get_mint_amount(value_to_mint_for)?;

        let next_minted_value = minted_fee_value
            .checked_add(delta_minted_value)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        store.gt_mut().mint_to(user, minted)?;

        self.gt_reward = minted;
        user.gt.paid_fee_value = next_paid_fee_value;
        user.gt.minted_fee_value = next_minted_value;

        event_emitter
            .emit_cpi(&GtUpdated::minted(
                minting_cost,
                minted,
                store.gt(),
                Some(user),
            ))
            .expect("failed to emit GT minted event");

        Ok(())
    }

    pub(crate) fn update(&mut self, id: u64, params: &UpdateOrderParams) -> Result<()> {
        let current = &mut self.params;
        require!(current.is_updatable()?, CoreError::InvalidArgument);
        require!(!params.is_empty(), CoreError::InvalidArgument);

        self.header.id = id;

        if let Some(size_delta_value) = params.size_delta_value {
            current.size_delta_value = size_delta_value;
        }

        if let Some(acceptable_price) = params.acceptable_price {
            current.acceptable_price = acceptable_price;
        }

        if let Some(trigger_price) = params.trigger_price {
            current.trigger_price = trigger_price;
        }

        if let Some(min_output) = params.min_output {
            if matches!(current.kind()?, OrderKind::LimitSwap) {
                require_neq!(min_output, 0, CoreError::InvalidArgument);
            }
            current.min_output = min_output;
        }

        if let Some(ts) = params.valid_from_ts {
            current.valid_from_ts = ts;
        }

        self.header.updated()?;

        Ok(())
    }
}

/// Token accounts for Order.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OrderTokenAccounts {
    /// Initial collateral.
    pub(crate) initial_collateral: TokenAndAccount,
    /// Final output token.
    pub(crate) final_output_token: TokenAndAccount,
    /// Long token.
    pub(crate) long_token: TokenAndAccount,
    /// Short token.
    pub(crate) short_token: TokenAndAccount,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl OrderTokenAccounts {
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
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OrderActionParams {
    /// Kind.
    kind: u8,
    /// Order side.
    side: u8,
    /// Decrease position swap type.
    decrease_position_swap_type: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 5],
    /// Collateral/Output token.
    pub(crate) collateral_token: Pubkey,
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
    pub(crate) valid_from_ts: i64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_2: [u8; 8],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl OrderActionParams {
    const DEFAULT_VALID_FROM_TS: i64 = 0;

    pub(crate) fn init_swap(
        &mut self,
        kind: OrderKind,
        collateral_token: Pubkey,
        swap_in_amount: u64,
        min_output: Option<u128>,
        valid_from_ts: Option<i64>,
    ) -> Result<()> {
        self.kind = kind.into();
        self.collateral_token = collateral_token;
        self.initial_collateral_delta_amount = swap_in_amount;
        match kind {
            OrderKind::MarketSwap => {
                self.min_output = min_output.unwrap_or(0);
                self.valid_from_ts = Self::DEFAULT_VALID_FROM_TS;
            }
            OrderKind::LimitSwap => {
                let Some(min_output) = min_output else {
                    return err!(CoreError::InvalidMinOutputAmount);
                };
                require!(min_output != 0, CoreError::Internal);
                self.min_output = min_output;

                self.valid_from_ts = valid_from_ts.unwrap_or(Self::DEFAULT_VALID_FROM_TS);
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
        min_output: Option<u128>,
        valid_from_ts: Option<i64>,
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
        self.min_output = min_output.unwrap_or(0);
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
                self.valid_from_ts = Self::DEFAULT_VALID_FROM_TS;
            }
            OrderKind::LimitIncrease => {
                let Some(price) = trigger_price else {
                    return err!(CoreError::InvalidTriggerPrice);
                };
                self.trigger_price = price;
                self.valid_from_ts = valid_from_ts.unwrap_or(Self::DEFAULT_VALID_FROM_TS);
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
        swap_type: DecreasePositionSwapType,
        valid_from_ts: Option<i64>,
    ) -> Result<()> {
        self.kind = kind.into();
        self.side = if is_long {
            OrderSide::Long
        } else {
            OrderSide::Short
        }
        .into();
        self.decrease_position_swap_type = swap_type.into();
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
                self.valid_from_ts = Self::DEFAULT_VALID_FROM_TS;
            }
            OrderKind::LimitDecrease | OrderKind::StopLossDecrease => {
                let Some(price) = trigger_price else {
                    return err!(CoreError::InvalidTriggerPrice);
                };
                self.trigger_price = price;
                self.valid_from_ts = valid_from_ts.unwrap_or(Self::DEFAULT_VALID_FROM_TS);
            }
            _ => {
                return err!(CoreError::Internal);
            }
        }
        Ok(())
    }

    /// Get order kind.
    pub fn kind(&self) -> Result<OrderKind> {
        self.kind
            .try_into()
            .map_err(|_| error!(CoreError::UnknownOrderKind))
    }

    /// Get decrease position swap type.
    pub fn decrease_position_swap_type(&self) -> Result<DecreasePositionSwapType> {
        let ty = self
            .decrease_position_swap_type
            .try_into()
            .map_err(|_| CoreError::UnknownDecreasePositionSwapType)?;
        Ok(ty)
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
        self.side
            .try_into()
            .map_err(|_| error!(CoreError::UnknownOrderSide))
    }

    /// Get position address.
    pub fn position(&self) -> Option<&Pubkey> {
        optional_address(&self.position)
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

    /// Get valid from ts.
    pub fn valid_from_ts(&self) -> i64 {
        self.valid_from_ts
    }
}
