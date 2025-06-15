use anchor_lang::prelude::*;
use borsh::BorshSerialize;
use gmsol_utils::InitSpace;

use crate::{
    events::Event,
    states::{order::TransferOut, position::PositionState},
};

use super::{TradeData, TradeFees, TradeOutputAmounts, TradePnl, TradePrice, TradePrices};

#[cfg(feature = "utils")]
use crate::states::Position;

#[cfg(feature = "utils")]
use gmsol_utils::order::{TradeFlag, TradeFlagContainer};

/// This is a cheaper variant of [`TradeEvent`], sharing the same format
/// for serialization.
#[derive(Clone, BorshSerialize)]
pub(crate) struct TradeEventRef<'a>(&'a TradeData);

impl<'a> From<&'a TradeData> for TradeEventRef<'a> {
    fn from(value: &'a TradeData) -> Self {
        Self(value)
    }
}

impl anchor_lang::Discriminator for TradeEventRef<'_> {
    const DISCRIMINATOR: &'static [u8] = TradeEvent::DISCRIMINATOR;
}

impl InitSpace for TradeEventRef<'_> {
    // The borsh init space of `TradeData` is used here.
    const INIT_SPACE: usize = <TradeData as anchor_lang::Space>::INIT_SPACE;
}

impl Event for TradeEventRef<'_> {}

static_assertions::const_assert_eq!(TradeEventRef::<'static>::INIT_SPACE, TradeEvent::INIT_SPACE);

/// Trade event.
#[event]
#[derive(Clone, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TradeEvent {
    /// Trade flag.
    // Note: The concrete type can be replaced with the type alias `TradeFlag`.
    // However, this will cause the IDL build to fail in `anchor v0.30.1`.
    pub flags: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding_0: [u8; 7],
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
    pub before: EventPositionState,
    /// After state.
    pub after: EventPositionState,
    /// Transfer out.
    pub transfer_out: EventTransferOut,
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding_1: [u8; 8],
    /// Prices.
    pub prices: EventTradePrices,
    /// Execution price.
    pub execution_price: u128,
    /// Price impact value.
    pub price_impact_value: i128,
    /// Price impact diff.
    pub price_impact_diff: u128,
    /// Processed pnl.
    pub pnl: EventTradePnl,
    /// Fees.
    pub fees: EventTradeFees,
    /// Output amounts.
    #[cfg_attr(feature = "serde", serde(default))]
    pub output_amounts: EventTradeOutputAmounts,
}

#[cfg(feature = "utils")]
impl TradeEvent {
    /// Get trade data flag.
    pub fn get_flag(&self, flag: TradeFlag) -> bool {
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

        position
            .try_init(
                kind,
                // Note: there's no need to provide a correct bump here for now.
                0,
                self.store,
                &self.user,
                &self.market_token,
                &collateral_token,
            )
            .unwrap();
        position.state = self.after.clone().into();
        position
    }
}

#[cfg(feature = "display")]
impl std::fmt::Display for TradeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::utils::pubkey::optional_address;

        f.debug_struct("TradeEvent")
            .field("trade_id", &self.trade_id)
            .field("store", &self.store.to_string())
            .field("market_token", &self.market_token.to_string())
            .field("user", &self.user.to_string())
            .field("position", &self.position.to_string())
            .field("order", &self.order.to_string())
            .field(
                "final_output_token",
                &optional_address(&self.final_output_token),
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

/// Position State.
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(AnchorDeserialize, AnchorSerialize, InitSpace, Clone)]
pub struct EventPositionState {
    /// Trade id.
    pub trade_id: u64,
    /// The time that the position last increased at.
    pub increased_at: i64,
    /// Updated at slot.
    pub updated_at_slot: u64,
    /// The time that the position last decreased at.
    pub decreased_at: i64,
    /// Size in tokens.
    pub size_in_tokens: u128,
    /// Collateral amount.
    pub collateral_amount: u128,
    /// Size in usd.
    pub size_in_usd: u128,
    /// Borrowing factor.
    pub borrowing_factor: u128,
    /// Funding fee amount per size.
    pub funding_fee_amount_per_size: u128,
    /// Long token claimable funding amount per size.
    pub long_token_claimable_funding_amount_per_size: u128,
    /// Short token claimable funding amount per size.
    pub short_token_claimable_funding_amount_per_size: u128,
    /// Reserved.
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    pub(crate) reserved: [u8; 128],
}

static_assertions::const_assert_eq!(EventPositionState::INIT_SPACE, PositionState::INIT_SPACE);

/// Transfer Out.
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(AnchorSerialize, AnchorDeserialize, Default, InitSpace, Clone)]
pub struct EventTransferOut {
    /// Executed.
    pub executed: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding_0: [u8; 7],
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
    /// Short token amount for claimable account of user.
    pub short_token_for_claimable_account_of_user: u64,
    /// Long token amount for claimable account of holding.
    pub long_token_for_claimable_account_of_holding: u64,
    /// Short token amount for claimable account of holding.
    pub short_token_for_claimable_account_of_holding: u64,
}

static_assertions::const_assert_eq!(EventTransferOut::INIT_SPACE, TransferOut::INIT_SPACE);

/// Price.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone)]
pub struct EventTradePrice {
    /// Min price.
    pub min: u128,
    /// Max price.
    pub max: u128,
}

static_assertions::const_assert_eq!(EventTradePrice::INIT_SPACE, TradePrice::INIT_SPACE);

/// Trade Prices.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone)]
pub struct EventTradePrices {
    /// Index token price.
    pub index: EventTradePrice,
    /// Long token price.
    pub long: EventTradePrice,
    /// Short token price.
    pub short: EventTradePrice,
}

static_assertions::const_assert_eq!(EventTradePrices::INIT_SPACE, TradePrices::INIT_SPACE);

/// Trade PnL.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone)]
pub struct EventTradePnl {
    /// Final PnL value.
    pub pnl: i128,
    /// Uncapped PnL value.
    pub uncapped_pnl: i128,
}

static_assertions::const_assert_eq!(EventTradePnl::INIT_SPACE, TradePnl::INIT_SPACE);

/// Trade Fees.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone)]
pub struct EventTradeFees {
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

static_assertions::const_assert_eq!(EventTradeFees::INIT_SPACE, TradeFees::INIT_SPACE);

/// Output amounts.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[derive(AnchorSerialize, AnchorDeserialize, Default, InitSpace, Clone)]
pub struct EventTradeOutputAmounts {
    /// Output amount.
    pub output_amount: u128,
    /// Secondary output amount.
    pub secondary_output_amount: u128,
}

static_assertions::const_assert_eq!(
    EventTradeOutputAmounts::INIT_SPACE,
    TradeOutputAmounts::INIT_SPACE
);
