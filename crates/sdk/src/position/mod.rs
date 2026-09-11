use gmsol_model::{
    num::Unsigned, num_traits::Zero, price::Prices, PerpMarket, PerpMarketExt, Position,
    PositionExt, PositionState,
};
use gmsol_programs::model::PositionModel;
use status::PositionStatus;

use crate::constants;

/// Position status.
pub mod status;

/// Options for calculating position status.
#[derive(Debug, Clone, Default)]
pub struct CalculatePositionStatusOptions {
    /// Whether to include virtual inventory impact.
    pub include_virtual_inventory_impact: bool,
}

/// Position Calculations.
pub trait PositionCalculations {
    /// Calculate position status.
    fn status(&self, prices: &Prices<u128>) -> crate::Result<PositionStatus> {
        self.status_with_options(prices, Default::default())
    }

    /// Calculate position status with options.
    fn status_with_options(
        &self,
        prices: &Prices<u128>,
        options: CalculatePositionStatusOptions,
    ) -> crate::Result<PositionStatus>;
}

impl PositionCalculations for PositionModel {
    fn status_with_options(
        &self,
        prices: &Prices<u128>,
        options: CalculatePositionStatusOptions,
    ) -> crate::Result<PositionStatus> {
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
        let price_impact =
            self.position_price_impact(&size_delta_usd, options.include_virtual_inventory_impact)?;

        let mut price_impact_value = price_impact.value;
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
            price_impact.balance_change,
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
        let leverage = if !net_value.is_positive() {
            None
        } else {
            Some(
                gmsol_model::utils::div_to_factor::<_, { constants::MARKET_DECIMALS }>(
                    position_size_in_usd,
                    &net_value.unsigned_abs(),
                    true,
                )
                .ok_or(gmsol_model::Error::Computation("calculating leverage"))?,
            )
        };

        // liquidation price
        //
        // The threshold must come from `min_collateral_factor_for_liquidation`, which is what
        // `check_liquidatable(.., for_liquidation = true)` compares against on the liquidation
        // path (`crates/model/src/position.rs`). It falls back to `min_collateral_factor` when
        // the market leaves it unset, and `position_params()` already resolves the
        // market-closed variant, so reading it here covers both.
        let params = self.market().position_params()?;
        let min_collateral_factor = params.min_collateral_factor_for_liquidation();
        let min_collateral_value = params.min_collateral_value();
        let liquidation_collateral_usd = gmsol_model::utils::apply_factor::<
            _,
            { constants::MARKET_DECIMALS },
        >(position_size_in_usd, min_collateral_factor)
        .max(Some(*min_collateral_value))
        .ok_or(gmsol_model::Error::Computation(
            "calculating liquidation collateral usd",
        ))?;

        let liquidation_price = if position_size_in_tokens.is_zero() {
            None
        } else {
            collateral_value
                .checked_add_signed(price_impact_value)
                .and_then(|a| a.checked_sub(pending_borrowing_fee_value))
                .and_then(|a| a.checked_sub(pending_funding_fee_value))
                .and_then(|a| a.checked_sub(close_order_fee_value))
                .and_then(|remaining_collateral_usd| {
                    if self.is_long() {
                        liquidation_collateral_usd
                            .checked_add(*position_size_in_usd)?
                            .checked_sub(remaining_collateral_usd)?
                            .checked_div(*position_size_in_tokens)
                    } else {
                        remaining_collateral_usd
                            .checked_add(*position_size_in_usd)?
                            .checked_sub(liquidation_collateral_usd)?
                            .checked_div(*position_size_in_tokens)
                    }
                })
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

#[cfg(test)]
mod tests {
    use super::*;
    use gmsol_model::price::Price;
    use gmsol_programs::{
        anchor_lang::prelude::Pubkey,
        bytemuck::Zeroable,
        gmsol_store::accounts::{Market, Position},
        model::MarketModel,
    };
    use std::sync::Arc;

    // Units. A USD value carries MARKET_DECIMALS (1e20); a token amount carries
    // MARKET_TOKEN_DECIMALS (1e9); a price is scaled so amount * price lands back in USD,
    // i.e. 1e11 per 1 USD. Getting this wrong overflows u128 inside collateral_value.
    const USD: u128 = constants::MARKET_USD_UNIT;
    const PRICE: u128 = constants::MARKET_USD_TO_AMOUNT_DIVISOR;
    const TOKEN: u128 = 10u128.pow(constants::MARKET_TOKEN_DECIMALS as u32);

    const SIZE_USD: u128 = 10_000 * USD;
    const SIZE_TOKENS: u128 = 500 * TOKEN; // entry 20 USD
    const COLLATERAL: u128 = 1_000 * TOKEN; // in the short token, priced at 1 USD

    /// A market carrying two *different* collateral factors, which is what every mainnet
    /// market actually looks like.  is the one the
    /// contract compares against on the liquidation path.
    fn market(min_collateral_factor: u128, for_liquidation: u128) -> MarketModel {
        let mut m = Market::zeroed();
        m.meta.market_token_mint = Pubkey::new_unique();
        m.meta.index_token_mint = Pubkey::new_unique();
        m.meta.long_token_mint = Pubkey::new_unique();
        m.meta.short_token_mint = Pubkey::new_unique();
        m.config.min_collateral_factor = min_collateral_factor;
        m.config.min_collateral_factor_for_liquidation = for_liquidation;
        m.config.min_collateral_value = 0;
        // A zeroed market has no open interest, and closing the whole position underflows the
        // pool while computing price impact. Seed both long open-interest pools with the
        // position itself so the close nets to zero.
        m.state.pools.open_interest_for_long.pool.long_token_amount = SIZE_USD;
        m.state
            .pools
            .open_interest_in_tokens_for_long
            .pool
            .long_token_amount = SIZE_TOKENS;
        MarketModel::from_parts(Arc::new(m), 0)
    }

    /// Long, collateralised in the SHORT token so the collateral value does not move with the
    /// index price; that isolates the factor from the separate same-token-collateral effect.
    fn position(market: &MarketModel) -> PositionModel {
        let mut p = Position::zeroed();
        p.kind = 1; // Long
        p.collateral_token = market.meta.short_token_mint;
        p.state.size_in_usd = SIZE_USD;
        p.state.size_in_tokens = SIZE_TOKENS;
        p.state.collateral_amount = COLLATERAL;
        PositionModel::new(market.clone(), Arc::new(p)).expect("position model")
    }

    fn prices() -> Prices<u128> {
        let one = Price {
            min: PRICE,
            max: PRICE,
        };
        let index = Price {
            min: 20 * PRICE,
            max: 20 * PRICE,
        };
        Prices {
            index_token_price: index,
            long_token_price: one,
            short_token_price: one,
        }
    }

    /// long boundary: (threshold + size_in_usd - collateral_value) / size_in_tokens,
    /// with no fees or price impact on a zeroed market.
    fn expected(factor: u128) -> u128 {
        let threshold = SIZE_USD / USD * factor;
        let collateral_value = COLLATERAL * PRICE;
        (threshold + SIZE_USD - collateral_value) / SIZE_TOKENS
    }

    #[test]
    fn liquidation_price_uses_the_liquidation_factor_not_the_plain_one() {
        // 1% vs 0.5%, the ratio 89 of 101 mainnet markets carry.
        let mcf = USD / 100;
        let liq = USD / 200;
        let reported = position(&market(mcf, liq))
            .status(&prices())
            .expect("status")
            .liquidation_price
            .expect("liquidation price");

        assert_eq!(
            reported,
            expected(liq),
            "liquidation_price must be built from min_collateral_factor_for_liquidation"
        );
        assert_ne!(
            reported,
            expected(mcf),
            "regression guard: this is the value the old code produced"
        );
        // 18.10 rather than 18.20, the worked example on the issue
        // 18.10 rather than 18.20, matching the worked example on the issue
        assert_eq!(reported * 100 / PRICE, 1810, "expected 18.10");
        assert_eq!(expected(mcf) * 100 / PRICE, 1820, "the old code gave 18.20");
    }

    #[test]
    fn falls_back_to_min_collateral_factor_when_the_market_leaves_it_unset() {
        let mcf = USD / 100;
        let reported = position(&market(mcf, 0)) // 0 means unset; the accessor falls back
            .status(&prices())
            .expect("status")
            .liquidation_price
            .expect("liquidation price");
        assert_eq!(reported, expected(mcf));
    }
}
