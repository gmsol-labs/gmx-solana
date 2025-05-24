use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::Zeroable;
use gmsol_model::{
    action::{
        decrease_position::DecreasePositionReport, increase_position::IncreasePositionReport,
    },
    params::fee::PositionFees,
    price::Prices,
};
use gmsol_utils::InitSpace;

use crate::{
    states::{order::TransferOut, position::PositionState, Position, Seed},
    utils::pubkey::DEFAULT_PUBKEY,
    CoreError,
};

/// Trade Data Flags.
#[allow(clippy::enum_variant_names)]
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum TradeFlag {
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
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
pub struct TradeData {
    /// Trade flag.
    // Note: The concrete type can be replaced with the type alias `TradeFlag`.
    // However, this will cause the IDL build to fail in `anchor v0.30.1`.
    flags: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
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
    #[cfg_attr(feature = "debug", debug(skip))]
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
    pub pnl: TradePnl,
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
pub struct TradePnl {
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
        require_keys_eq!(self.store, position.store, CoreError::PermissionDenied);
        self.market_token = position.market_token;
        self.user = position.owner;
        self.position = pubkey;
        self.order = order;
        self.final_output_token = DEFAULT_PUBKEY;
        self.ts = clock.unix_timestamp;
        self.slot = clock.slot;
        self.before = position.state;
        self.after = position.state;
        self.transfer_out = TransferOut::zeroed();
        self.prices = TradePrices::zeroed();
        self.execution_price = 0;
        self.price_impact_value = 0;
        self.price_impact_diff = 0;
        self.pnl = TradePnl::zeroed();
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

#[cfg(test)]
mod tests {

    #[test]
    #[cfg(feature = "utils")]
    fn test_trade_event() {
        use crate::events::{
            EventPositionState, EventTradeFees, EventTradeOutputAmounts, EventTradePnl,
            EventTradePrice, EventTradePrices, EventTransferOut, TradeEvent, TradeEventRef,
        };

        use super::*;

        let state = EventPositionState {
            trade_id: u64::MAX,
            increased_at: i64::MAX,
            updated_at_slot: u64::MAX,
            decreased_at: i64::MAX,
            size_in_tokens: u128::MAX,
            collateral_amount: u128::MAX,
            size_in_usd: u128::MAX,
            borrowing_factor: u128::MAX,
            funding_fee_amount_per_size: u128::MAX,
            long_token_claimable_funding_amount_per_size: u128::MAX,
            short_token_claimable_funding_amount_per_size: u128::MAX,
            reserved: [0; 128],
        };

        let transfer_out = EventTransferOut {
            executed: u8::MAX,
            padding_0: Default::default(),
            final_output_token: u64::MAX,
            secondary_output_token: u64::MAX,
            long_token: u64::MAX,
            short_token: u64::MAX,
            long_token_for_claimable_account_of_user: u64::MAX,
            short_token_for_claimable_account_of_user: u64::MAX,
            long_token_for_claimable_account_of_holding: u64::MAX,
            short_token_for_claimable_account_of_holding: u64::MAX,
        };

        let price = EventTradePrice {
            min: u128::MAX,
            max: u128::MAX,
        };

        let event = TradeEvent {
            flags: u8::MAX,
            padding_0: Default::default(),
            trade_id: u64::MAX,
            authority: Pubkey::new_unique(),
            store: Pubkey::new_unique(),
            market_token: Pubkey::new_unique(),
            user: Pubkey::new_unique(),
            position: Pubkey::new_unique(),
            order: Pubkey::new_unique(),
            final_output_token: Pubkey::new_unique(),
            ts: i64::MAX,
            slot: u64::MAX,
            before: state.clone(),
            after: state,
            transfer_out,
            padding_1: Default::default(),
            prices: EventTradePrices {
                index: price.clone(),
                long: price.clone(),
                short: price.clone(),
            },
            execution_price: u128::MAX,
            price_impact_value: i128::MAX,
            price_impact_diff: u128::MAX,
            pnl: EventTradePnl {
                pnl: i128::MAX,
                uncapped_pnl: i128::MAX,
            },
            fees: EventTradeFees {
                order_fee_for_receiver_amount: u128::MAX,
                order_fee_for_pool_amount: u128::MAX,
                liquidation_fee_amount: u128::MAX,
                liquidation_fee_for_receiver_amount: u128::MAX,
                total_borrowing_fee_amount: u128::MAX,
                borrowing_fee_for_receiver_amount: u128::MAX,
                funding_fee_amount: u128::MAX,
                claimable_funding_fee_long_token_amount: u128::MAX,
                claimable_funding_fee_short_token_amount: u128::MAX,
            },
            output_amounts: EventTradeOutputAmounts {
                output_amount: u128::MAX,
                secondary_output_amount: u128::MAX,
            },
        };

        let TradeEvent {
            flags,
            padding_0,
            trade_id,
            authority,
            store,
            market_token,
            user,
            position,
            order,
            final_output_token,
            ts,
            slot,
            before,
            after,
            transfer_out,
            padding_1,
            prices: _,
            execution_price,
            price_impact_value,
            price_impact_diff,
            pnl,
            fees,
            output_amounts,
        } = event.clone();

        let price = TradePrice {
            min: price.min,
            max: price.max,
        };
        let data = TradeData {
            flags,
            padding_0,
            trade_id,
            authority,
            store,
            market_token,
            user,
            position,
            order,
            final_output_token,
            ts,
            slot,
            before: before.into(),
            after: after.into(),
            transfer_out: transfer_out.into(),
            padding_1,
            prices: TradePrices {
                index: price,
                long: price,
                short: price,
            },
            execution_price,
            price_impact_value,
            price_impact_diff,
            pnl: TradePnl {
                pnl: pnl.pnl,
                uncapped_pnl: pnl.uncapped_pnl,
            },
            fees: TradeFees {
                order_fee_for_receiver_amount: fees.order_fee_for_receiver_amount,
                order_fee_for_pool_amount: fees.order_fee_for_pool_amount,
                liquidation_fee_amount: fees.liquidation_fee_amount,
                liquidation_fee_for_receiver_amount: fees.liquidation_fee_for_receiver_amount,
                total_borrowing_fee_amount: fees.total_borrowing_fee_amount,
                borrowing_fee_for_receiver_amount: fees.borrowing_fee_for_receiver_amount,
                funding_fee_amount: fees.funding_fee_amount,
                claimable_funding_fee_long_token_amount: fees
                    .claimable_funding_fee_long_token_amount,
                claimable_funding_fee_short_token_amount: fees
                    .claimable_funding_fee_short_token_amount,
            },
            output_amounts: TradeOutputAmounts {
                output_amount: output_amounts.output_amount,
                secondary_output_amount: output_amounts.secondary_output_amount,
            },
        };

        let mut serialized_event = Vec::with_capacity(TradeEvent::INIT_SPACE);
        event
            .serialize(&mut serialized_event)
            .expect("serializing `TradeEvent`");

        let mut serialized_data = Vec::with_capacity(<TradeData as anchor_lang::Space>::INIT_SPACE);
        TradeEventRef::from(&data)
            .serialize(&mut serialized_data)
            .expect("serializing `TradeEventRef`");

        assert_eq!(serialized_event, serialized_data);
    }
}
