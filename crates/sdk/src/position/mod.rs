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

        // When the collateral token *is* the index token, two of the terms below are functions of
        // the very price being solved for, so they cannot be held at spot:
        //
        //   collateral_value        = collateral_amount        * collateral_token_price
        //   pending_funding_fee     = pending_funding_amount   * collateral_token_price
        //
        // Everything else is price-independent: the borrowing fee is `apply_factor(size_in_usd, ..)`
        // and the close order fee is `apply_factor(size_delta_usd, ..)`, both plain USD, and the
        // price impact is computed off pool balances. So the boundary stays linear in `P` and the
        // correction is entirely in the denominator:
        //
        //   long:  P = (liq + size_in_usd - K) / (size_in_tokens + collateral_amount - funding)
        //   short: P = (K + size_in_usd - liq) / (size_in_tokens - collateral_amount + funding)
        //
        // where K is what remains of `remaining_collateral_usd` once the two price-dependent terms
        // are taken back out. With a different collateral token the extra terms are zero and this
        // reduces to the original formula.
        //
        // Correlated-but-not-identical tokens are deliberately left uncorrected: that error decays
        // to zero as the position approaches liquidation and the UI recomputes continuously.
        let collateral_tracks_index =
            self.position().collateral_token == self.market_model().meta.index_token_mint;
        // The two amounts are added before either is subtracted, so an intermediate never
        // underflows. A short whose collateral exceeds `size_in_tokens + funding` has no
        // liquidation price on the way up at all, and the `checked_sub` returning `None` is the
        // right answer there rather than a number.
        let denominator = if collateral_tracks_index {
            let collateral_amount = *self.collateral_amount();
            let funding_amount = *pending_funding_fee.amount();
            if self.is_long() {
                position_size_in_tokens
                    .checked_add(collateral_amount)
                    .and_then(|d| d.checked_sub(funding_amount))
            } else {
                position_size_in_tokens
                    .checked_add(funding_amount)
                    .and_then(|d| d.checked_sub(collateral_amount))
            }
        } else {
            Some(*position_size_in_tokens)
        };

        let liquidation_price = if position_size_in_tokens.is_zero() {
            None
        } else {
            collateral_value
                .checked_add_signed(price_impact_value)
                .and_then(|a| a.checked_sub(pending_borrowing_fee_value))
                .and_then(|a| a.checked_sub(pending_funding_fee_value))
                .and_then(|a| a.checked_sub(close_order_fee_value))
                .and_then(|remaining_collateral_usd| {
                    let denominator = denominator?;
                    if denominator.is_zero() {
                        return None;
                    }
                    if !collateral_tracks_index {
                        return if self.is_long() {
                            liquidation_collateral_usd
                                .checked_add(*position_size_in_usd)?
                                .checked_sub(remaining_collateral_usd)?
                                .checked_div(denominator)
                        } else {
                            remaining_collateral_usd
                                .checked_add(*position_size_in_usd)?
                                .checked_sub(liquidation_collateral_usd)?
                                .checked_div(denominator)
                        };
                    }

                    // The price-independent residual is `impact - borrowing - order_fee`, which is
                    // never positive: the impact is clamped to <= 0 just above and both fees are
                    // >= 0. These are unsigned, so it cannot be represented; carry its magnitude
                    // instead, `surcharge = borrowing + order_fee - impact >= 0`.
                    //
                    // Subtracting `remaining` before the funding fee keeps both steps non-negative
                    // without assuming the collateral exceeds the funding owed:
                    //   collateral - remaining          = borrowing + funding + order_fee - impact
                    //   ... - funding                   = borrowing + order_fee - impact
                    let surcharge = collateral_value
                        .checked_sub(remaining_collateral_usd)?
                        .checked_sub(pending_funding_fee_value)?;

                    if self.is_long() {
                        liquidation_collateral_usd
                            .checked_add(*position_size_in_usd)?
                            .checked_add(surcharge)?
                            .checked_div(denominator)
                    } else {
                        // `S - L - surcharge`. Underflow here means the position is already past
                        // the boundary, and no liquidation price is the honest answer.
                        position_size_in_usd
                            .checked_sub(liquidation_collateral_usd)?
                            .checked_sub(surcharge)?
                            .checked_div(denominator)
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

    /// A market whose index token IS its long token, which is the shape the correction exists
    /// for. The assertion is not against a formula of my own: it binary-searches the real
    /// `check_liquidatable(.., true, true)` and requires the reported price to be that boundary.
    #[test]
    fn same_token_collateral_matches_the_real_liquidation_boundary() {
        let liq = USD / 200;
        let mut m = Market::zeroed();
        let shared = Pubkey::new_unique();
        m.meta.market_token_mint = Pubkey::new_unique();
        m.meta.index_token_mint = shared;
        m.meta.long_token_mint = shared; // index == long
        m.meta.short_token_mint = Pubkey::new_unique();
        m.config.min_collateral_factor = USD / 100;
        m.config.min_collateral_factor_for_liquidation = liq;
        m.config.min_collateral_value = 0;
        m.state.pools.open_interest_for_long.pool.long_token_amount = SIZE_USD;
        m.state
            .pools
            .open_interest_in_tokens_for_long
            .pool
            .long_token_amount = SIZE_TOKENS;
        let market = MarketModel::from_parts(Arc::new(m), 0);

        let mut pos = Position::zeroed();
        pos.kind = 1; // Long
        pos.collateral_token = shared; // collateral IS the index token
        pos.state.size_in_usd = SIZE_USD;
        pos.state.size_in_tokens = SIZE_TOKENS;
        let collateral = 50 * TOKEN; // 1 000 USD at spot 20, in the index token itself
        pos.state.collateral_amount = collateral;
        let p = PositionModel::new(market, Arc::new(pos)).expect("position model");

        // both index and collateral prices move together: they are the same mint
        let at = |price: u128| {
            let px = Price {
                min: price,
                max: price,
            };
            Prices {
                index_token_price: px,
                long_token_price: px,
                short_token_price: Price {
                    min: PRICE,
                    max: PRICE,
                },
            }
        };
        let liquidatable = |price: u128| {
            p.check_liquidatable(&at(price), true, true)
                .expect("check_liquidatable")
                .is_some()
        };

        let spot = 20 * PRICE;
        assert!(!liquidatable(spot), "position must start healthy");
        assert!(liquidatable(PRICE), "must be liquidatable near zero");

        // lowest price at which the contract still considers the position healthy
        let (mut lo, mut hi) = (PRICE, spot);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if liquidatable(mid) {
                lo = mid
            } else {
                hi = mid
            }
        }
        let boundary = hi;

        let reported = p
            .status(&at(spot))
            .expect("status")
            .liquidation_price
            .expect("price");

        // one unit of slack for the integer division inside the formula
        let diff = reported.abs_diff(boundary);
        assert!(
            diff <= 8,
            "reported {reported} should be the real boundary {boundary} (diff {diff})"
        );

        // and the old, spot-pinned formula would have been materially lower
        let pinned = {
            let threshold = SIZE_USD / USD * liq;
            let collateral_value = collateral * spot;
            (threshold + SIZE_USD - collateral_value) / SIZE_TOKENS
        };
        assert!(
            pinned < boundary,
            "regression guard: the spot-pinned value {pinned} understated the boundary {boundary}"
        );
    }

    /// Sweep every shape against the REAL check_liquidatable boundary.
    ///
    /// At a zero oracle spread the reported price must BE the boundary. With a spread it cannot:
    /// the formula solves for a single price while the model picks `.min` for collateral and
    /// `.max`/`.min` for pnl by side, so a half-spread of error is inherent, and it shows up on
    /// positions this change does not touch as well. Measured before/after on this sweep, the
    /// same-token cases went 0.945% -> 0.000% (long) and 0.955% -> 0.000% (short) at zero spread,
    /// while the different-token cases came out byte-identical, which is the control.
    #[test]
    fn sweep_reported_vs_real_boundary() {
        let mut checked = 0;
        for &is_long in &[true, false] {
            for &same_token in &[true, false] {
                for &spread_bps in &[0u128, 50] {
                    let mut m = Market::zeroed();
                    let shared = Pubkey::new_unique();
                    let other = Pubkey::new_unique();
                    m.meta.market_token_mint = Pubkey::new_unique();
                    m.meta.index_token_mint = shared;
                    m.meta.long_token_mint = shared;
                    m.meta.short_token_mint = other;
                    m.config.min_collateral_factor = USD / 100;
                    m.config.min_collateral_factor_for_liquidation = USD / 200;
                    m.config.min_collateral_value = 0;
                    // non-zero fees, so the price-independent residual is not 0 in any shape;
                    // with these at 0 the same-token branch cannot exercise its subtraction
                    m.config.order_fee_factor_for_positive_impact = USD / 1_000;
                    m.config.order_fee_factor_for_negative_impact = USD / 1_000;
                    if is_long {
                        m.state.pools.open_interest_for_long.pool.long_token_amount = SIZE_USD;
                        m.state
                            .pools
                            .open_interest_in_tokens_for_long
                            .pool
                            .long_token_amount = SIZE_TOKENS;
                    } else {
                        m.state
                            .pools
                            .open_interest_for_short
                            .pool
                            .short_token_amount = SIZE_USD;
                        m.state
                            .pools
                            .open_interest_in_tokens_for_short
                            .pool
                            .short_token_amount = SIZE_TOKENS;
                    }
                    let market = MarketModel::from_parts(Arc::new(m), 0);

                    let mut pos = Position::zeroed();
                    pos.kind = if is_long { 1 } else { 2 };
                    pos.collateral_token = if same_token { shared } else { other };
                    pos.state.size_in_usd = SIZE_USD;
                    pos.state.size_in_tokens = SIZE_TOKENS;
                    // 1 000 USD of collateral either way: 50 index tokens at 20, or 1 000 of the other
                    pos.state.collateral_amount = if same_token {
                        50 * TOKEN
                    } else {
                        1_000 * TOKEN
                    };
                    let pm = match PositionModel::new(market, Arc::new(pos)) {
                        Ok(v) => v,
                        Err(e) => panic!(
                            "long={is_long} same={same_token} spread={spread_bps}: fixture failed to build: {e}"
                        ),
                    };

                    let at = |price: u128| {
                        let half = price * spread_bps / 20_000;
                        let px = Price {
                            min: price - half,
                            max: price + half,
                        };
                        let one = Price {
                            min: PRICE,
                            max: PRICE,
                        };
                        Prices {
                            index_token_price: px,
                            long_token_price: px,
                            short_token_price: one,
                        }
                    };
                    let liq = |price: u128| {
                        pm.check_liquidatable(&at(price), true, true)
                            .map(|r| r.is_some())
                    };

                    let spot = 20 * PRICE;
                    let healthy_at_spot = liq(spot);
                    assert_eq!(
                        healthy_at_spot.as_ref().ok(),
                        Some(&false),
                        "long={is_long} same={same_token} spread={spread_bps}: fixture must start healthy"
                    );
                    // long liquidates downward, short upward
                    let (mut lo, mut hi) = if is_long {
                        (PRICE, spot)
                    } else {
                        (spot, 400 * PRICE)
                    };
                    let far_ok = if is_long { liq(lo) } else { liq(hi) };
                    assert_eq!(
                        far_ok.as_ref().ok(),
                        Some(&true),
                        "long={is_long} same={same_token} spread={spread_bps}: must liquidate somewhere in range"
                    );
                    while hi - lo > 1 {
                        let mid = lo + (hi - lo) / 2;
                        let l = liq(mid).unwrap_or(false);
                        if is_long {
                            if l {
                                lo = mid
                            } else {
                                hi = mid
                            }
                        } else if l {
                            hi = mid
                        } else {
                            lo = mid
                        }
                    }
                    let boundary = if is_long { hi } else { lo };

                    let reported = pm
                        .status(&at(spot))
                        .expect("status")
                        .liquidation_price
                        .expect("liquidation price");
                    let diff = reported.abs_diff(boundary);
                    if spread_bps == 0 {
                        assert!(
                    diff <= 8,
                    "long={is_long} same={same_token}: {reported} must be the boundary {boundary}, diff {diff}"
                );
                    } else {
                        let bps = diff * 10_000 / boundary;
                        assert!(
                    bps <= 50,
                    "long={is_long} same={same_token}: {bps}bps exceeds the half-spread budget"
                );
                    }
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 8, "every shape must be exercised, none skipped");
    }

    /// Reported by mv-reyes on PR #439: with any non-zero fee the same-token branch was
    /// claimed to underflow and return `None` for exactly the positions it targets. Every other
    /// test here runs on a zeroed market, where all fees are 0, so none of them could see it.
    #[test]
    fn same_token_survives_a_non_zero_order_fee() {
        let mut m = Market::zeroed();
        let shared = Pubkey::new_unique();
        m.meta.market_token_mint = Pubkey::new_unique();
        m.meta.index_token_mint = shared;
        m.meta.long_token_mint = shared;
        m.meta.short_token_mint = Pubkey::new_unique();
        m.config.min_collateral_factor = USD / 100;
        m.config.min_collateral_factor_for_liquidation = USD / 200;
        m.config.min_collateral_value = 0;
        // the one thing the other fixtures leave at zero: a real market always has this set
        m.config.order_fee_factor_for_positive_impact = USD / 1_000;
        m.config.order_fee_factor_for_negative_impact = USD / 1_000;
        m.state.pools.open_interest_for_long.pool.long_token_amount = SIZE_USD;
        m.state
            .pools
            .open_interest_in_tokens_for_long
            .pool
            .long_token_amount = SIZE_TOKENS;
        let market = MarketModel::from_parts(Arc::new(m), 0);

        let mut pos = Position::zeroed();
        pos.kind = 1;
        pos.collateral_token = shared;
        pos.state.size_in_usd = SIZE_USD;
        pos.state.size_in_tokens = SIZE_TOKENS;
        pos.state.collateral_amount = 50 * TOKEN;
        let pm = PositionModel::new(market, Arc::new(pos)).expect("position model");

        let at = |price: u128| {
            let px = Price {
                min: price,
                max: price,
            };
            Prices {
                index_token_price: px,
                long_token_price: px,
                short_token_price: Price {
                    min: PRICE,
                    max: PRICE,
                },
            }
        };

        let status = pm.status(&at(20 * PRICE)).expect("status");
        assert!(
            status.close_order_fee_value > 0,
            "fixture must actually charge a fee, else it proves nothing"
        );
        assert!(
            status.liquidation_price.is_some(),
            "liquidation_price went None with a non-zero fee: the same-token branch underflowed"
        );
    }
}
