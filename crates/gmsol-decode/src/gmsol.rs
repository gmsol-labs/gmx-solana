use crate::{impl_decode_for_cpi_event, impl_decode_for_zero_copy};

use gmsol_store::{
    events::{
        DepositExecuted, DepositRemoved, GlvDepositRemoved, GlvWithdrawalRemoved, GtUpdated,
        MarketFeesUpdated, MarketStateUpdated, OrderRemoved, PositionDecreased, PositionIncreased,
        ShiftRemoved, SwapExecuted, TradeEvent, WithdrawalExecuted, WithdrawalRemoved,
    },
    states::{
        Deposit, GlvDeposit, GlvShift, GlvWithdrawal, Market, Order, Position, Shift, Store,
        Withdrawal,
    },
};

use crate::{untagged, value::UnknownOwnedData};

impl_decode_for_zero_copy!(Store);
impl_decode_for_zero_copy!(Position);
impl_decode_for_zero_copy!(Market);
impl_decode_for_zero_copy!(Deposit);
impl_decode_for_zero_copy!(Withdrawal);
impl_decode_for_zero_copy!(Shift);
impl_decode_for_zero_copy!(Order);
impl_decode_for_zero_copy!(GlvDeposit);
impl_decode_for_zero_copy!(GlvWithdrawal);
impl_decode_for_zero_copy!(GlvShift);

impl_decode_for_cpi_event!(DepositRemoved);
impl_decode_for_cpi_event!(DepositExecuted);
impl_decode_for_cpi_event!(WithdrawalRemoved);
impl_decode_for_cpi_event!(WithdrawalExecuted);
impl_decode_for_cpi_event!(ShiftRemoved);
impl_decode_for_cpi_event!(GlvDepositRemoved);
impl_decode_for_cpi_event!(GlvWithdrawalRemoved);
impl_decode_for_cpi_event!(PositionIncreased);
impl_decode_for_cpi_event!(PositionDecreased);
impl_decode_for_cpi_event!(OrderRemoved);
impl_decode_for_cpi_event!(TradeEvent);
impl_decode_for_cpi_event!(MarketFeesUpdated);
impl_decode_for_cpi_event!(MarketStateUpdated);
impl_decode_for_cpi_event!(SwapExecuted);
impl_decode_for_cpi_event!(GtUpdated);

untagged!(
    GMSOLAccountData,
    [
        Deposit,
        Withdrawal,
        Shift,
        Order,
        GlvDeposit,
        GlvWithdrawal,
        GlvShift,
        Store,
        Market,
        Position,
        UnknownOwnedData
    ]
);

type Account = crate::value::Account<GMSOLAccountData>;

untagged!(
    GMSOLCPIEvent,
    [
        DepositExecuted,
        DepositRemoved,
        WithdrawalExecuted,
        WithdrawalRemoved,
        ShiftRemoved,
        GlvDepositRemoved,
        GlvWithdrawalRemoved,
        PositionIncreased,
        PositionDecreased,
        OrderRemoved,
        TradeEvent,
        MarketFeesUpdated,
        MarketStateUpdated,
        SwapExecuted,
        GtUpdated,
        UnknownOwnedData
    ]
);

type CPIEvents = crate::value::AnchorCPIEvents<GMSOLCPIEvent>;

untagged!(GMSOLData, [Account, CPIEvents]);
