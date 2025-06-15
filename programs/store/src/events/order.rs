use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use gmsol_model::action::{
    decrease_position::{DecreasePositionReport, DecreasePositionSwapType},
    increase_position::IncreasePositionReport,
};
use gmsol_utils::InitSpace;

use crate::states::{
    common::action::{Action, ActionState},
    order::OrderKind,
    Order,
};

use super::Event;

/// Order created event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct OrderCreated {
    /// Event time.
    pub ts: i64,
    /// Store account.
    pub store: Pubkey,
    /// Order account.
    pub order: Pubkey,
    /// Position account.
    pub position: Option<Pubkey>,
}

impl OrderCreated {
    pub(crate) fn new(store: Pubkey, order: Pubkey, position: Option<Pubkey>) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            order,
            position,
        })
    }
}

/// Position increased event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PositionIncreased {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: IncreasePositionReport<u128, i128>,
}

impl gmsol_utils::InitSpace for PositionIncreased {
    const INIT_SPACE: usize = 8 + 32 + IncreasePositionReport::<u128, i128>::INIT_SPACE;
}

impl Event for PositionIncreased {}

impl PositionIncreased {
    pub(crate) fn from_report(
        rev: u64,
        market_token: Pubkey,
        report: IncreasePositionReport<u128, i128>,
    ) -> Self {
        Self {
            rev,
            market_token,
            report,
        }
    }
}

/// Position decrease event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PositionDecreased {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: Box<DecreasePositionReport<u128, i128>>,
}

impl gmsol_utils::InitSpace for PositionDecreased {
    const INIT_SPACE: usize = 8 + 32 + DecreasePositionReport::<u128, i128>::INIT_SPACE;
}

impl Event for PositionDecreased {}

impl PositionDecreased {
    pub(crate) fn from_report(
        rev: u64,
        market_token: Pubkey,
        report: Box<DecreasePositionReport<u128, i128>>,
    ) -> Self {
        Self {
            rev,
            market_token,
            report,
        }
    }
}

/// Order removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct OrderRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Order.
    pub order: Pubkey,
    /// Kind.
    pub kind: OrderKind,
    /// Market token.
    pub market_token: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Final state.
    pub state: ActionState,
    /// Reason.
    #[max_len(32)]
    pub reason: String,
}

impl OrderRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        order: Pubkey,
        kind: OrderKind,
        market_token: Pubkey,
        owner: Pubkey,
        state: ActionState,
        reason: impl ToString,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            id,
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store,
            kind,
            order,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for OrderRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for OrderRemoved {}

/// An event indicating that insufficient funding fee payment has occurred.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct InsufficientFundingFeePayment {
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Market token.
    pub market_token: Pubkey,
    /// Funding fee amount to pay.
    pub cost_amount: u128,
    /// Paid collateral token amount.
    pub paid_in_collateral_amount: u128,
    /// Paid secondary token amount.
    pub paid_in_secondary_output_amount: u128,
    /// Whether the collateral token is long token.
    pub is_collateral_token_long: bool,
}

impl InsufficientFundingFeePayment {
    pub(crate) fn new(
        store: &Pubkey,
        market_token: &Pubkey,
        cost_amount: u128,
        paid_in_collateral_amount: u128,
        paid_in_secondary_output_amount: u128,
        is_collateral_token_long: bool,
    ) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *store,
            market_token: *market_token,
            cost_amount,
            paid_in_collateral_amount,
            paid_in_secondary_output_amount,
            is_collateral_token_long,
        })
    }
}

impl InitSpace for InsufficientFundingFeePayment {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for InsufficientFundingFeePayment {}

/// Order parameters for event.
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct OrderParamsForEvent {
    /// Order kind.
    pub kind: OrderKind,
    /// Order side.
    pub is_long: bool,
    /// Decrease position swap type.
    pub decrease_position_swap_type: DecreasePositionSwapType,
    /// Position address.
    pub position: Option<Pubkey>,
    /// Collateral token.
    pub collateral_token: Pubkey,
    /// Initial collateral token.
    pub initial_collateral_token: Option<Pubkey>,
    /// Initial collateral delta amount.
    pub initial_collateral_delta_amount: u64,
    /// Size delta value.
    pub size_delta_value: u128,
    /// Min output.
    pub min_output: u128,
    /// Trigger price (in unit price).
    pub trigger_price: u128,
    /// Acceptable price (in unit price).
    pub acceptable_price: u128,
    /// Valid from this timestamp.
    pub valid_from_ts: i64,
}

impl TryFrom<&Order> for OrderParamsForEvent {
    type Error = Error;

    fn try_from(order: &Order) -> std::result::Result<Self, Self::Error> {
        let params = order.params();
        Ok(Self {
            kind: params.kind()?,
            is_long: params.side()?.is_long(),
            decrease_position_swap_type: params.decrease_position_swap_type()?,
            position: params.position().copied(),
            collateral_token: params.collateral_token,
            initial_collateral_token: order.tokens.initial_collateral.token(),
            initial_collateral_delta_amount: params.initial_collateral_delta_amount,
            size_delta_value: params.size_delta_value,
            min_output: params.min_output(),
            trigger_price: params.trigger_price,
            acceptable_price: params.acceptable_price,
            valid_from_ts: params.valid_from_ts,
        })
    }
}

/// An event indicating that an order is created or updated.
///
/// # Notes
/// - For compatibility reasons, the [`OrderUpdated`] event is not emitted
///   by the [`create_order`](crate::gmsol_store::create_order) and
///   [`update_order`](crate::gmsol_store::update_order) instructions.
///   As a result, there is no guarantee that every order will have
///   corresponding [`OrderUpdated`] events.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct OrderUpdated {
    /// Whether it is a create event.
    pub is_create: bool,
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Order.
    pub order: Pubkey,
    /// Market token.
    pub market_token: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Parameters.
    pub params: OrderParamsForEvent,
}

impl OrderUpdated {
    pub(crate) fn new(is_create: bool, address: &Pubkey, order: &Order) -> Result<Self> {
        let clock = Clock::get()?;
        Ok(Self {
            is_create,
            id: order.header().id(),
            ts: clock.unix_timestamp,
            slot: clock.slot,
            store: *order.header().store(),
            order: *address,
            market_token: *order.market_token(),
            owner: *order.header().owner(),
            params: order.try_into()?,
        })
    }
}

impl InitSpace for OrderUpdated {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl Event for OrderUpdated {}
