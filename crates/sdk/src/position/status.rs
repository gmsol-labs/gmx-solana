use gmsol_model::{
    num::Unsigned, price::Prices, PerpMarket, PerpMarketExt, Position, PositionExt, PositionState,
};

/// Position Status.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct PositionStatus {
    /// Entry price.
    pub entry_price: u128,
    /// Collateral value.
    pub collateral_value: u128,
    /// Pending PnL.
    pub pending_pnl: i128,
    /// Pending borrowing fee value.
    pub pending_borrowing_fee_value: u128,
    /// Pending funding fee value.
    pub pending_funding_fee_value: u128,
    /// Pending claimable funding fee value in long token.
    pub pending_claimable_funding_fee_value_in_long_token: u128,
    /// Pending claimable funding fee value in short token.
    pub pending_claimable_funding_fee_value_in_short_token: u128,
    /// Close order fee value.
    pub close_order_fee_value: u128,
    /// Net value.
    pub net_value: i128,
    /// Leverage.
    pub leverage: Option<i128>,
    /// Liquidation price.
    pub liquidation_price: Option<i128>,
}

use gmsol_programs::model::PositionModel;
use rust_decimal::prelude::Zero;

use super::PositionCalculations;

impl PositionCalculations for PositionModel {
    fn status(&self, prices: &Prices<u128>) -> crate::Result<PositionStatus> {
        // collateral value
        let collateral_value = self.collateral_value(prices)?;

        // pnl
        let position_size_in_tokens = self.size_in_tokens();
        let position_size_in_usd = self.size_in_usd();
        let _position_size_in_usd_real = position_size_in_tokens
            .checked_mul(prices.index_token_price.max)
            .ok_or(gmsol_model::Error::Computation(
                "calculating position size in usd real",
            ))?;
        let (pending_pnl_value, _uncapped_pnl_value, _size_delta_in_tokens) =
            self.pnl_value(prices, position_size_in_usd)?;
        let entry_price = position_size_in_usd
            .checked_div(*position_size_in_tokens)
            .ok_or(gmsol_model::Error::Computation("calculating entry price"))?;

        // borrowing fee value
        let pending_borrowing_fee_value = self.pending_borrowing_fee_value()?;

        // funding fee value
        let pending_funding_fee = self.pending_funding_fees()?;
        let pending_funding_fee_value = if self.is_collateral_token_long() {
            pending_funding_fee
                .amount()
                .checked_mul(prices.long_token_price.min)
                .ok_or(gmsol_model::Error::Computation(
                    "calculating pending funding fee value",
                ))?
        } else {
            pending_funding_fee
                .amount()
                .checked_mul(prices.short_token_price.min)
                .ok_or(gmsol_model::Error::Computation(
                    "calculating pending funding fee value",
                ))?
        };
        let pending_claimable_funding_fee_value_in_long_token = pending_funding_fee
            .claimable_long_token_amount()
            .checked_mul(prices.long_token_price.min)
            .ok_or(gmsol_model::Error::Computation(
                "calculating pending claimable funding fee value in long token",
            ))?;
        let pending_claimable_funding_fee_value_in_short_token = pending_funding_fee
            .claimable_short_token_amount()
            .checked_mul(prices.short_token_price.min)
            .ok_or(gmsol_model::Error::Computation(
                "calculating pending claimable funding fee value in short token",
            ))?;

        // close order fee value
        let collateral_token_price = if self.is_collateral_token_long() {
            prices.long_token_price
        } else {
            prices.short_token_price
        };

        // net value = collateral value +  pending pnl - pending borrowing fee value - nagetive pending funding fee value - close order fee value let mut price_impact_value = self.position_price_impact(&size_delta_usd)?;
        let size_delta_usd = position_size_in_usd.to_opposite_signed()?;
        let mut price_impact_value = self.position_price_impact(&size_delta_usd)?;
        let has_positive_impact = price_impact_value.is_positive();
        if price_impact_value.is_negative() {
            self.market().cap_negative_position_price_impact(
                &size_delta_usd,
                true,
                &mut price_impact_value,
            )?;
        } else {
            price_impact_value = Zero::zero();
        }

        let total_position_fees = self.position_fees(
            &collateral_token_price,
            position_size_in_usd,
            has_positive_impact,
            // Should not account for liquidation fees to determine if position should be liquidated.
            false,
        )?;

        let close_order_fee_value = *total_position_fees.order_fees().fee_value();

        let net_value = collateral_value
            .to_signed()?
            .checked_add(pending_pnl_value)
            .ok_or(gmsol_model::Error::Computation("calculating net value"))?
            .checked_sub(pending_borrowing_fee_value.to_signed()?)
            .ok_or(gmsol_model::Error::Computation("calculating net value"))?
            .checked_sub(pending_funding_fee_value.to_signed()?)
            .ok_or(gmsol_model::Error::Computation("calculating net value"))?
            .checked_sub(close_order_fee_value.to_signed()?)
            .ok_or(gmsol_model::Error::Computation("calculating net value"))?
            .max(Zero::zero());

        // leverage
        let leverage = if net_value.is_zero() {
            None
        } else {
            Some(
                position_size_in_usd
                    .to_signed()?
                    .checked_div(net_value)
                    .ok_or(gmsol_model::Error::Computation("calculating leverage"))?,
            )
        };

        // liquidation price

        let params = self.market().position_params()?;
        let min_collateral_factor = params.min_collateral_factor();
        let min_collateral_value = params.min_collateral_value();
        let liquidation_collateral_usd = position_size_in_usd
            .checked_mul(*min_collateral_factor)
            .max(Some(*min_collateral_value))
            .ok_or(gmsol_model::Error::Computation(
                "calculating liquidation collateral usd",
            ))?;

        let liquidation_price = if position_size_in_tokens.is_zero() {
            None
        } else {
            let remaining_collateral_usd = collateral_value
                .to_signed()?
                .checked_add(price_impact_value)
                .ok_or(gmsol_model::Error::Computation(
                    "calculating remaining collateral usd",
                ))?
                .checked_sub(pending_borrowing_fee_value.to_signed()?)
                .ok_or(gmsol_model::Error::Computation("calculating net value"))?
                .checked_sub(pending_funding_fee_value.to_signed()?)
                .ok_or(gmsol_model::Error::Computation("calculating net value"))?
                .checked_sub(close_order_fee_value.to_signed()?)
                .ok_or(gmsol_model::Error::Computation("calculating net value"))?;

            let lq = if self.is_collateral_token_long() {
                liquidation_collateral_usd
                    .to_signed()?
                    .checked_sub(remaining_collateral_usd)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
                    .checked_add(position_size_in_usd.to_signed()?)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
                    .checked_div(position_size_in_tokens.to_signed()?)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
            } else {
                remaining_collateral_usd
                    .checked_add(position_size_in_usd.to_signed()?)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
                    .checked_sub(liquidation_collateral_usd.to_signed()?)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
                    .checked_div(position_size_in_tokens.to_signed()?)
                    .ok_or(gmsol_model::Error::Computation(
                        "calculating liquidation price",
                    ))?
            };
            Some(lq)
        };

        Ok(PositionStatus {
            entry_price,
            collateral_value,
            pending_pnl: pending_pnl_value,
            pending_borrowing_fee_value,
            pending_funding_fee_value,
            pending_claimable_funding_fee_value_in_long_token,
            pending_claimable_funding_fee_value_in_short_token,
            close_order_fee_value,
            net_value,
            leverage,
            liquidation_price,
        })
    }
}
