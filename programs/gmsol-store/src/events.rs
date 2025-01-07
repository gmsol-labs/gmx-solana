use std::{borrow::Cow, ops::Deref};

use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::Zeroable;
use gmsol_model::{
    action::{
        decrease_position::{DecreasePositionReport, DecreasePositionSwapType},
        deposit::DepositReport,
        distribute_position_impact::DistributePositionImpactReport,
        increase_position::IncreasePositionReport,
        swap::SwapReport,
        update_borrowing_state::UpdateBorrowingReport,
        update_funding_state::UpdateFundingReport,
        withdraw::WithdrawReport,
    },
    params::fee::PositionFees,
    price::Prices,
};
use gmsol_utils::InitSpace;

use crate::{
    states::{
        common::action::{ActionEvent, ActionState},
        gt::GtState,
        order::{OrderKind, TransferOut},
        position::PositionState,
        Position, Seed,
    },
    CoreError,
};

/// Deposit Created Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DepositCreated {
    /// Event time.
    pub ts: i64,
    /// Store account.
    pub store: Pubkey,
    /// Deposit account.
    pub deposit: Pubkey,
}

impl DepositCreated {
    pub(crate) fn new(store: Pubkey, deposit: Pubkey) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            deposit,
        })
    }
}

/// Deposit executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct DepositExecuted {
    /// Report.
    pub report: DepositReport<u128, i128>,
}

impl gmsol_utils::InitSpace for DepositExecuted {
    const INIT_SPACE: usize = DepositReport::<u128, i128>::INIT_SPACE;
}

impl ActionEvent for DepositExecuted {}

impl From<DepositReport<u128, i128>> for DepositExecuted {
    fn from(report: DepositReport<u128, i128>) -> Self {
        Self { report }
    }
}

/// Deposit removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct DepositRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Deposit.
    pub deposit: Pubkey,
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

impl DepositRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        deposit: Pubkey,
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
            deposit,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for DepositRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for DepositRemoved {}

/// Withdrawal created event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct WithdrawalCreated {
    /// Event time.
    pub ts: i64,
    /// Store account.
    pub store: Pubkey,
    /// Withdrawal account.
    pub withdrawal: Pubkey,
}

impl WithdrawalCreated {
    pub(crate) fn new(store: Pubkey, withdrawal: Pubkey) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            withdrawal,
        })
    }
}

/// Withdrawal executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct WithdrawalExecuted {
    /// Report.
    pub report: WithdrawReport<u128>,
}

impl gmsol_utils::InitSpace for WithdrawalExecuted {
    const INIT_SPACE: usize = WithdrawReport::<u128>::INIT_SPACE;
}

impl ActionEvent for WithdrawalExecuted {}

impl From<WithdrawReport<u128>> for WithdrawalExecuted {
    fn from(report: WithdrawReport<u128>) -> Self {
        Self { report }
    }
}

/// Withdrawal removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct WithdrawalRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Withdrawal.
    pub withdrawal: Pubkey,
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

impl InitSpace for WithdrawalRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for WithdrawalRemoved {}

impl WithdrawalRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        withdrawal: Pubkey,
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
            withdrawal,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

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
    /// Report.
    pub report: IncreasePositionReport<u128, i128>,
}

impl gmsol_utils::InitSpace for PositionIncreased {
    const INIT_SPACE: usize = IncreasePositionReport::<u128, i128>::INIT_SPACE;
}

impl ActionEvent for PositionIncreased {}

impl From<IncreasePositionReport<u128, i128>> for PositionIncreased {
    fn from(report: IncreasePositionReport<u128, i128>) -> Self {
        Self { report }
    }
}

/// Position decrease event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct PositionDecreased {
    /// Report.
    pub report: Box<DecreasePositionReport<u128, i128>>,
}

impl gmsol_utils::InitSpace for PositionDecreased {
    const INIT_SPACE: usize = DecreasePositionReport::<u128, i128>::INIT_SPACE;
}

impl ActionEvent for PositionDecreased {}

impl From<Box<DecreasePositionReport<u128, i128>>> for PositionDecreased {
    fn from(report: Box<DecreasePositionReport<u128, i128>>) -> Self {
        Self { report }
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

impl ActionEvent for OrderRemoved {}

/// Shift removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct ShiftRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// Shift.
    pub shift: Pubkey,
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

impl ShiftRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        shift: Pubkey,
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
            shift,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for ShiftRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for ShiftRemoved {}

/// GLV Deposit removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct GlvDepositRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// GLV Deposit.
    pub glv_deposit: Pubkey,
    /// Market token.
    pub market_token: Pubkey,
    /// GLV token.
    pub glv_token: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Final state.
    pub state: ActionState,
    /// Reason.
    #[max_len(32)]
    pub reason: String,
}

impl GlvDepositRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        glv_deposit: Pubkey,
        market_token: Pubkey,
        glv_token: Pubkey,
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
            glv_deposit,
            glv_token,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for GlvDepositRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for GlvDepositRemoved {}

/// GLV Withdrawal removed event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(Clone, InitSpace)]
pub struct GlvWithdrawalRemoved {
    /// Action id.
    pub id: u64,
    /// Timestamp.
    pub ts: i64,
    /// Slot.
    pub slot: u64,
    /// Store.
    pub store: Pubkey,
    /// GLV Withdrawal
    pub glv_withdrawal: Pubkey,
    /// Market token.
    pub market_token: Pubkey,
    /// GLV token.
    pub glv_token: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Final state.
    pub state: ActionState,
    /// Reason.
    #[max_len(32)]
    pub reason: String,
}

impl GlvWithdrawalRemoved {
    pub(crate) fn new(
        id: u64,
        store: Pubkey,
        glv_withdrawal: Pubkey,
        market_token: Pubkey,
        glv_token: Pubkey,
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
            glv_withdrawal,
            glv_token,
            market_token,
            owner,
            state,
            reason: reason.to_string(),
        })
    }
}

impl InitSpace for GlvWithdrawalRemoved {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for GlvWithdrawalRemoved {}

/// A trade.
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Trade<'a>(Cow<'a, TradeData>);

/// Only used to generate the discriminator for [`Trade`].
/// Please refer to [`TradeData`] for the actual structure definition.
#[event]
struct TradeEvent;

impl<'a> From<&'a TradeData> for Trade<'a> {
    fn from(value: &'a TradeData) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl<'a> anchor_lang::Event for Trade<'a> {
    fn data(&self) -> Vec<u8> {
        use anchor_lang::Discriminator;

        let mut data = Vec::with_capacity(256);
        data.extend_from_slice(&TradeEvent::DISCRIMINATOR);
        // Borsh serialization is used here to align with other events.
        self.serialize(&mut data).unwrap();
        data
    }
}

impl<'a> anchor_lang::Discriminator for Trade<'a> {
    const DISCRIMINATOR: [u8; 8] = TradeEvent::DISCRIMINATOR;
}

impl<'a> InitSpace for Trade<'a> {
    // The borsh init space of `TradeData` is used here.
    const INIT_SPACE: usize = <TradeData as anchor_lang::Space>::INIT_SPACE;
}

impl<'a> ActionEvent for Trade<'a> {}

#[cfg(feature = "display")]
impl<'a> std::fmt::Display for Trade<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TradeEvent")
            .field("trade_id", &self.trade_id)
            .field("store", &self.store.to_string())
            .field("market_token", &self.market_token.to_string())
            .field("user", &self.user.to_string())
            .field("position", &self.position.to_string())
            .field("order", &self.order.to_string())
            .field(
                "final_output_token",
                &(self.final_output_token == Pubkey::default()).then_some(self.final_output_token),
            )
            .field("ts", &self.ts)
            .field("slot", &self.slot)
            .field("is_long", &self.is_long())
            .field("is_collateral_long", &self.is_collateral_long())
            .field("is_increase", &self.is_increase())
            .field("delta_collateral_amount", &self.delta_collateral_amount())
            .field("delta_size_in_usd", &self.delta_size_in_usd())
            .field("delta_size_in_tokens", &self.delta_size_in_tokens())
            .field("prices", &self.prices)
            .field("execution_price", &self.execution_price)
            .field("price_impact_value", &self.price_impact_value)
            .field("price_impact_diff", &self.price_impact_diff)
            .field("pnl", &self.pnl)
            .field("fees", &self.fees)
            .field("output_amounts", &self.output_amounts)
            .field("transfer_out", &self.transfer_out)
            .finish_non_exhaustive()
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
enum TradeFlag {
    /// Is long.
    IsLong,
    /// Is collateral long.
    IsCollateralLong,
    /// Is increase.
    IsIncrease,
    // CHECK: cannot have more than `8` flags.
}

gmsol_utils::flags!(TradeFlag, 8, u8);

/// Trade event data.
#[account(zero_copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradeData {
    /// Trade flag.
    // FIXME: Use the type alias `TradeFlag` instead of the concrete type.
    // However, this causes the IDL build to fail in `anchor v0.30.1`.
    flags: u8,
    padding_0: [u8; 7],
    /// Trade id.
    pub trade_id: u64,
    /// Authority.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub authority: Pubkey,
    /// Store address.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub store: Pubkey,
    /// Market token.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub market_token: Pubkey,
    /// User.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub user: Pubkey,
    /// Position address.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub position: Pubkey,
    /// Order address.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub order: Pubkey,
    /// Final output token.
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    pub final_output_token: Pubkey,
    /// Trade ts.
    pub ts: i64,
    /// Trade slot.
    pub slot: u64,
    /// Before state.
    pub before: PositionState,
    /// After state.
    pub after: PositionState,
    /// Transfer out.
    pub transfer_out: TransferOut,
    padding_1: [u8; 8],
    /// Prices.
    pub prices: TradePrices,
    /// Execution price.
    pub execution_price: u128,
    /// Price impact value.
    pub price_impact_value: i128,
    /// Price impact diff.
    pub price_impact_diff: u128,
    /// Processed pnl.
    pub pnl: TradePnL,
    /// Fees.
    pub fees: TradeFees,
    /// Output amounts.
    #[cfg_attr(feature = "serde", serde(default))]
    pub output_amounts: TradeOutputAmounts,
}

impl InitSpace for TradeData {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for TradeData {
    const SEED: &'static [u8] = b"trade_event_data";
}

/// Price.
#[zero_copy]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradePrice {
    /// Min price.
    pub min: u128,
    /// Max price.
    pub max: u128,
}

/// Prices.
#[zero_copy]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradePrices {
    /// Index token price.
    pub index: TradePrice,
    /// Long token price.
    pub long: TradePrice,
    /// Short token price.
    pub short: TradePrice,
}

impl TradePrices {
    fn set_with_prices(&mut self, prices: &Prices<u128>) {
        self.index.min = prices.index_token_price.min;
        self.index.max = prices.index_token_price.max;
        self.long.min = prices.long_token_price.min;
        self.long.max = prices.long_token_price.max;
        self.short.min = prices.short_token_price.min;
        self.short.max = prices.short_token_price.max;
    }
}

/// Trade PnL.
#[zero_copy]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradePnL {
    /// Final PnL value.
    pub pnl: i128,
    /// Uncapped PnL value.
    pub uncapped_pnl: i128,
}

/// Trade Fees.
#[zero_copy]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradeFees {
    /// Order fee for receiver amount.
    pub order_fee_for_receiver_amount: u128,
    /// Order fee for pool amount.
    pub order_fee_for_pool_amount: u128,
    /// Total liquidation fee amount.
    pub liquidation_fee_amount: u128,
    /// Liquidation fee for pool amount.
    pub liquidation_fee_for_receiver_amount: u128,
    /// Total borrowing fee amount.
    pub total_borrowing_fee_amount: u128,
    /// Borrowing fee for receiver amount.
    pub borrowing_fee_for_receiver_amount: u128,
    /// Funding fee amount.
    pub funding_fee_amount: u128,
    /// Claimable funding fee long token amount.
    pub claimable_funding_fee_long_token_amount: u128,
    /// Claimable funding fee short token amount.
    pub claimable_funding_fee_short_token_amount: u128,
}

impl TradeFees {
    fn set_with_position_fees(&mut self, fees: &PositionFees<u128>) {
        self.order_fee_for_receiver_amount =
            *fees.order_fees().fee_amounts().fee_amount_for_receiver();
        self.order_fee_for_pool_amount = *fees.order_fees().fee_amounts().fee_amount_for_pool();
        if let Some(fees) = fees.liquidation_fees() {
            self.liquidation_fee_amount = *fees.fee_amount();
            self.liquidation_fee_for_receiver_amount = *fees.fee_amount_for_receiver();
        }
        self.total_borrowing_fee_amount = *fees.borrowing_fees().fee_amount();
        self.borrowing_fee_for_receiver_amount = *fees.borrowing_fees().fee_amount_for_receiver();
        self.funding_fee_amount = *fees.funding_fees().amount();
        self.claimable_funding_fee_long_token_amount =
            *fees.funding_fees().claimable_long_token_amount();
        self.claimable_funding_fee_short_token_amount =
            *fees.funding_fees().claimable_short_token_amount();
    }
}

/// Output amounts.
#[zero_copy]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(BorshSerialize, BorshDeserialize, Default, InitSpace)]
pub struct TradeOutputAmounts {
    /// Output amount.
    pub output_amount: u128,
    /// Secondary output amount.
    pub secondary_output_amount: u128,
}

impl TradeData {
    pub(crate) fn init(
        &mut self,
        is_increase: bool,
        is_collateral_long: bool,
        pubkey: Pubkey,
        position: &Position,
        order: Pubkey,
    ) -> Result<&mut Self> {
        let clock = Clock::get()?;
        self.set_flags(position.try_is_long()?, is_collateral_long, is_increase);
        self.trade_id = 0;
        require_eq!(self.store, position.store, CoreError::PermissionDenied);
        self.market_token = position.market_token;
        self.user = position.owner;
        self.position = pubkey;
        self.order = order;
        self.final_output_token = Pubkey::default();
        self.ts = clock.unix_timestamp;
        self.slot = clock.slot;
        self.before = position.state;
        self.after = position.state;
        self.transfer_out = TransferOut::zeroed();
        self.prices = TradePrices::zeroed();
        self.execution_price = 0;
        self.price_impact_value = 0;
        self.price_impact_diff = 0;
        self.pnl = TradePnL::zeroed();
        self.fees = TradeFees::zeroed();
        self.output_amounts = TradeOutputAmounts::zeroed();
        Ok(self)
    }

    fn set_flags(
        &mut self,
        is_long: bool,
        is_collateral_long: bool,
        is_increase: bool,
    ) -> &mut Self {
        let mut flags = TradeFlagContainer::default();
        flags.set_flag(TradeFlag::IsLong, is_long);
        flags.set_flag(TradeFlag::IsCollateralLong, is_collateral_long);
        flags.set_flag(TradeFlag::IsIncrease, is_increase);
        self.flags = flags.into_value();
        self
    }

    fn get_flag(&self, flag: TradeFlag) -> bool {
        let map = TradeFlagContainer::from_value(self.flags);
        map.get_flag(flag)
    }

    /// Return whether the position side is long.
    pub fn is_long(&self) -> bool {
        self.get_flag(TradeFlag::IsLong)
    }

    /// Return whether the collateral side is long.
    pub fn is_collateral_long(&self) -> bool {
        self.get_flag(TradeFlag::IsCollateralLong)
    }

    /// Return whether the trade is caused by an increase order.
    pub fn is_increase(&self) -> bool {
        self.get_flag(TradeFlag::IsIncrease)
    }

    fn validate(&self) -> Result<()> {
        require_gt!(
            self.trade_id,
            self.before.trade_id,
            CoreError::InvalidTradeID
        );
        if self.is_increase() {
            require_gte!(
                self.after.size_in_usd,
                self.before.size_in_usd,
                CoreError::InvalidTradeDeltaSize
            );
            require_gte!(
                self.after.size_in_tokens,
                self.before.size_in_tokens,
                CoreError::InvalidTradeDeltaTokens
            );
        } else {
            require_gte!(
                self.before.size_in_usd,
                self.after.size_in_usd,
                CoreError::InvalidTradeDeltaSize
            );
            require_gte!(
                self.before.size_in_tokens,
                self.after.size_in_tokens,
                CoreError::InvalidTradeDeltaTokens
            );
        }
        require_gte!(
            self.after.borrowing_factor,
            self.before.borrowing_factor,
            CoreError::InvalidBorrowingFactor
        );
        require_gte!(
            self.after.funding_fee_amount_per_size,
            self.before.funding_fee_amount_per_size,
            CoreError::InvalidFundingFactors
        );
        require_gte!(
            self.after.long_token_claimable_funding_amount_per_size,
            self.before.long_token_claimable_funding_amount_per_size,
            CoreError::InvalidFundingFactors
        );
        require_gte!(
            self.after.short_token_claimable_funding_amount_per_size,
            self.before.short_token_claimable_funding_amount_per_size,
            CoreError::InvalidFundingFactors
        );
        Ok(())
    }

    /// Update with new position state.
    pub(crate) fn update_with_state(&mut self, new_state: &PositionState) -> Result<()> {
        self.trade_id = new_state.trade_id;
        self.after = *new_state;
        self.validate()?;
        Ok(())
    }

    /// Update with transfer out.
    #[inline(never)]
    pub(crate) fn update_with_transfer_out(&mut self, transfer_out: &TransferOut) -> Result<()> {
        self.transfer_out = *transfer_out;
        self.transfer_out.set_executed(true);
        Ok(())
    }

    pub(crate) fn set_final_output_token(&mut self, token: &Pubkey) {
        self.final_output_token = *token;
    }

    /// Update with increase report.
    #[inline(never)]
    pub(crate) fn update_with_increase_report(
        &mut self,
        report: &IncreasePositionReport<u128, i128>,
    ) -> Result<()> {
        self.prices.set_with_prices(report.params().prices());
        self.execution_price = *report.execution().execution_price();
        self.price_impact_value = *report.execution().price_impact_value();
        self.fees.set_with_position_fees(report.fees());
        Ok(())
    }

    /// Update with decrease report.
    pub(crate) fn update_with_decrease_report(
        &mut self,
        report: &DecreasePositionReport<u128, i128>,
        prices: &Prices<u128>,
    ) -> Result<()> {
        self.prices.set_with_prices(prices);
        self.execution_price = *report.execution_price();
        self.price_impact_value = *report.price_impact_value();
        self.price_impact_diff = *report.price_impact_diff();
        self.pnl.pnl = *report.pnl().pnl();
        self.pnl.uncapped_pnl = *report.pnl().uncapped_pnl();
        self.fees.set_with_position_fees(report.fees());
        self.output_amounts.output_amount = *report.output_amounts().output_amount();
        self.output_amounts.secondary_output_amount =
            *report.output_amounts().secondary_output_amount();
        Ok(())
    }
}

impl<'a> Deref for Trade<'a> {
    type Target = TradeData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "utils")]
impl<'a> Trade<'a> {
    /// Updated at.
    pub fn updated_at(&self) -> i64 {
        self.after.increased_at.max(self.after.decreased_at)
    }

    /// Delta size in usd.
    pub fn delta_size_in_usd(&self) -> u128 {
        self.after.size_in_usd.abs_diff(self.before.size_in_usd)
    }

    /// Delta size in tokens.
    pub fn delta_size_in_tokens(&self) -> u128 {
        self.after
            .size_in_tokens
            .abs_diff(self.before.size_in_tokens)
    }

    /// Delta collateral amount.
    pub fn delta_collateral_amount(&self) -> u128 {
        self.after
            .collateral_amount
            .abs_diff(self.before.collateral_amount)
    }

    /// Delta borrowing factor.
    pub fn delta_borrowing_factor(&self) -> u128 {
        self.after
            .borrowing_factor
            .abs_diff(self.before.borrowing_factor)
    }

    /// Delta funding fee amount per size.
    pub fn delta_funding_fee_amount_per_size(&self) -> u128 {
        self.after
            .funding_fee_amount_per_size
            .abs_diff(self.before.funding_fee_amount_per_size)
    }

    /// Funding fee amount.
    pub fn funding_fee(&self) -> u128 {
        self.delta_funding_fee_amount_per_size()
            .saturating_mul(self.before.size_in_usd)
    }

    /// Delta claimable amount per size.
    pub fn delta_claimable_funding_amount_per_size(&self, is_long_token: bool) -> u128 {
        if is_long_token {
            self.after
                .long_token_claimable_funding_amount_per_size
                .abs_diff(self.before.long_token_claimable_funding_amount_per_size)
        } else {
            self.after
                .short_token_claimable_funding_amount_per_size
                .abs_diff(self.before.short_token_claimable_funding_amount_per_size)
        }
    }

    #[cfg(feature = "utils")]
    /// Create position from this event.
    pub fn to_position(&self, meta: &impl crate::states::HasMarketMeta) -> Position {
        use crate::states::position::PositionKind;

        let mut position = Position::default();

        let kind = if self.is_long() {
            PositionKind::Long
        } else {
            PositionKind::Short
        };

        let collateral_token = if self.is_collateral_long() {
            meta.market_meta().long_token_mint
        } else {
            meta.market_meta().short_token_mint
        };

        // FIXME: should we provide a correct bump here?
        position
            .try_init(
                kind,
                0,
                self.store,
                &self.user,
                &self.market_token,
                &collateral_token,
            )
            .unwrap();
        position.state = self.after;
        position
    }
}

/// Market state updated event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct MarketStateUpdated {
    /// Position impact distribution report.
    pub position_impact_distribution: DistributePositionImpactReport<u128>,
    /// Update borrowing state report.
    pub update_borrowing_state: UpdateBorrowingReport<u128>,
    /// Update funding state report.
    pub update_funding_state: UpdateFundingReport<u128, i128>,
}

impl gmsol_utils::InitSpace for MarketStateUpdated {
    const INIT_SPACE: usize = DistributePositionImpactReport::<u128>::INIT_SPACE
        + UpdateBorrowingReport::<u128>::INIT_SPACE
        + UpdateFundingReport::<u128, i128>::INIT_SPACE;
}

impl ActionEvent for MarketStateUpdated {}

impl MarketStateUpdated {
    /// Create from reports.
    pub fn from_reports(
        position_impact_distribution: DistributePositionImpactReport<u128>,
        update_borrowing_state: UpdateBorrowingReport<u128>,
        update_funding_state: UpdateFundingReport<u128, i128>,
    ) -> Self {
        Self {
            position_impact_distribution,
            update_borrowing_state,
            update_funding_state,
        }
    }
}

/// Swap executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct SwapExecuted {
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: SwapReport<u128, i128>,
    /// Type.
    pub ty: Option<DecreasePositionSwapType>,
}

impl gmsol_utils::InitSpace for SwapExecuted {
    const INIT_SPACE: usize =
        SwapReport::<u128, i128>::INIT_SPACE + 1 + DecreasePositionSwapType::INIT_SPACE;
}

impl ActionEvent for SwapExecuted {}

impl SwapExecuted {
    /// Create.
    pub fn new(
        market_token: Pubkey,
        report: SwapReport<u128, i128>,
        ty: Option<DecreasePositionSwapType>,
    ) -> Self {
        Self {
            market_token,
            report,
            ty,
        }
    }
}

/// GT updated event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(InitSpace)]
pub struct GtUpdated {
    /// Initiator.
    pub initiator: Pubkey,
    /// Whether the GT is minted or burned.
    pub is_minted: bool,
    /// Whether the GT is reward.
    pub is_reward: bool,
    /// Delta.
    pub delta: u64,
    /// Minting cost.
    pub minting_cost: u128,
    /// Total minted.
    pub total_minted: u64,
    /// Grow steps.
    pub grow_steps: u64,
    /// Latest supply.
    pub supply: u64,
    /// Vault.
    pub vault: u64,
    reserved: [u8; 64],
}

impl gmsol_utils::InitSpace for GtUpdated {
    const INIT_SPACE: usize = <Self as Space>::INIT_SPACE;
}

impl ActionEvent for GtUpdated {}

impl GtUpdated {
    fn new(
        initiator: Pubkey,
        minting_cost: Option<u128>,
        is_reward: bool,
        delta: u64,
        state: &GtState,
    ) -> Self {
        Self {
            initiator,
            is_minted: minting_cost.is_some(),
            is_reward,
            delta,
            minting_cost: minting_cost.unwrap_or(state.minting_cost()),
            total_minted: state.total_minted(),
            grow_steps: state.grow_steps(),
            supply: state.supply(),
            vault: state.gt_vault(),
            reserved: [0; 64],
        }
    }

    /// Create a new rewarded event.
    pub fn rewarded(initiator: Pubkey, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, Some(state.minting_cost()), true, amount, state)
    }

    /// Create a new minted event.
    pub fn minted(initiator: Pubkey, minting_cost: u128, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, Some(minting_cost), false, amount, state)
    }

    /// Create a new burned event.
    pub fn burned(initiator: Pubkey, amount: u64, state: &GtState) -> Self {
        Self::new(initiator, None, false, amount, state)
    }
}
