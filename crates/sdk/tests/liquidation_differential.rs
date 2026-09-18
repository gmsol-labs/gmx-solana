//! Differential sweep: `PositionStatus::liquidation_price` vs the contract's
//! liquidation condition.
//!
//! For every generated market/position configuration the harness binary
//! searches the real liquidation boundary with the same call the program makes
//! on the liquidation path (`check_liquidatable(.., true, true)`), then
//! requires the reported price to agree with it. Each case also records which
//! shapes it actually reached (side, collateral kind, impact sign, fee
//! activity, floor engagement, outcome) and the sweep fails loudly if any
//! required shape was never exercised, so a generator regression that quietly
//! narrows coverage cannot pass.
//!
//! The oracle is given a positive control per case: the binary search result
//! is only trusted after `check_liquidatable` is observed flipping on both
//! sides of the found boundary.
//!
//! Run: `cargo test -p gmsol-sdk --test liquidation_differential -- --nocapture`

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use gmsol_sdk::constants;
use gmsol_sdk::model::num::Unsigned;
use gmsol_sdk::model::price::{Price, Prices};
use gmsol_sdk::model::PositionExt;
use gmsol_sdk::position::PositionCalculations;
use gmsol_sdk::programs::anchor_lang::prelude::Pubkey;
use gmsol_sdk::programs::bytemuck::Zeroable;
use gmsol_sdk::programs::gmsol_store::accounts::{Market, Position};
use gmsol_sdk::programs::model::{MarketModel, PositionModel};
use sol_rpc_mini::det::{fixture_bytes, Rng};

// Units. A USD value carries MARKET_DECIMALS (1e20); a token amount carries
// MARKET_TOKEN_DECIMALS (1e9); a price is scaled so amount * price lands back
// in USD, i.e. 1e11 per 1 USD.
const USD: u128 = constants::MARKET_USD_UNIT;
const PRICE: u128 = constants::MARKET_USD_TO_AMOUNT_DIVISOR;
const TOKEN: u128 = 10u128.pow(constants::MARKET_TOKEN_DECIMALS as u32);

const SIZE_USD: u128 = 10_000 * USD;
const SIZE_TOKENS: u128 = 500 * TOKEN; // entry 20 USD
const SPOT: u128 = 20 * PRICE;

/// Deterministic mint address for a fixture label.
fn mint(label: &str) -> Pubkey {
    Pubkey::new_from_array(fixture_bytes(label))
}

/// How the open-interest pools are imbalanced, which sets the sign of the
/// price impact on a full close.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ImpactShape {
    /// Pools net to zero: no impact.
    Neutral,
    /// Own side heavier: the close improves balance (positive impact, which
    /// both the status path and the liquidation check clamp to zero).
    PositiveClamped,
    /// Opposite side heavier: the close worsens balance (negative impact).
    Negative,
    /// Impact factors unset (fresh market config): impact is exactly zero
    /// regardless of imbalance.
    ZeroFactor,
}

/// The `liquidation_price = None` outcomes worth pinning down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NoneShape {
    /// Reported price expected.
    Priced,
    /// Collateral far under the threshold: already liquidatable at spot.
    DeepUnderwater,
    /// Same-token short whose collateral out-gains the position on the way
    /// up: the denominator underflows and `None` is the honest answer.
    UnwinnableShort,
}

struct CaseParams {
    label: String,
    is_long: bool,
    same_token_collateral: bool,
    spread_bps: u128,
    order_fee_bps: u128,
    borrowing_fee_bps: u128,
    funding_fee_bps: u128,
    impact: ImpactShape,
    /// min_collateral_value as a percentage of size * liq_factor; <= 100 means
    /// the floor never binds.
    floor_pct: u128,
    /// collateral value vs the liquidation threshold at spot, in percent.
    collateral_pct_of_threshold: u128,
    none_shape: NoneShape,
}

impl fmt::Display for CaseParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{} {} spread={}bps fees(o={},b={},f={})bps impact={:?} floor={}% coll={}%]",
            self.label,
            if self.is_long { "long" } else { "short" },
            if self.same_token_collateral { "same" } else { "diff" },
            self.spread_bps,
            self.order_fee_bps,
            self.borrowing_fee_bps,
            self.funding_fee_bps,
            self.impact,
            self.floor_pct,
            self.collateral_pct_of_threshold,
        )
    }
}

/// Reachability ledger: which shapes the sweep actually exercised. The point
/// of the harness is that a sweep that never reached a shape fails loudly.
#[derive(Default)]
struct Ledger(BTreeMap<&'static str, u32>);

impl Ledger {
    fn hit(&mut self, shape: &'static str) {
        *self.0.entry(shape).or_insert(0) += 1;
    }

    fn assert_coverage(&self, required: &[(&'static str, u32)]) {
        let mut missing = Vec::new();
        for (shape, min) in required {
            let got = self.0.get(shape).copied().unwrap_or(0);
            if got < *min {
                missing.push(format!("{shape}: {got} reached, {min} required"));
            }
        }
        assert!(
            missing.is_empty(),
            "sweep never exercised required shapes:\n  {}",
            missing.join("\n  ")
        );
    }
}

impl fmt::Display for Ledger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (shape, count) in &self.0 {
            writeln!(f, "  {shape}: {count}")?;
        }
        Ok(())
    }
}

struct Fixture {
    model: gmsol_sdk::programs::model::PositionModel,
    liquidation_collateral_usd: u128,
    floor_engaged: bool,
}

fn build_fixture(cfg: &CaseParams) -> Fixture {
    let index = mint("mint/index");
    let short = mint("mint/short");

    let liq_factor = USD / 200; // 0.50%, the ratio most mainnet markets carry
    let size_factor_threshold = SIZE_USD / USD * liq_factor;
    let min_collateral_value = if cfg.floor_pct > 100 {
        size_factor_threshold * cfg.floor_pct / 100
    } else {
        0
    };
    let liquidation_collateral_usd = size_factor_threshold.max(min_collateral_value);
    let floor_engaged = min_collateral_value > size_factor_threshold;

    let mut m = Market::zeroed();
    m.meta.market_token_mint = mint("mint/market");
    m.meta.index_token_mint = index;
    m.meta.long_token_mint = index; // index token doubles as the long token
    m.meta.short_token_mint = short;
    m.config.min_collateral_factor = USD / 100;
    m.config.min_collateral_factor_for_liquidation = liq_factor;
    m.config.min_collateral_value = min_collateral_value;

    // Order fees on both impact signs, so the fee term is live regardless of
    // which way the close tips the pools.
    let order_fee = USD * cfg.order_fee_bps / 10_000;
    m.config.order_fee_factor_for_positive_impact = order_fee;
    m.config.order_fee_factor_for_negative_impact = order_fee;

    // Impact factors. exponent = 1 keeps the impact linear in the imbalance
    // delta; the cap must be nonzero or `cap_negative_position_price_impact`
    // clamps every negative impact back to zero. ZeroFactor models a fresh
    // market with the factors unset, where impact is exactly zero.
    let (positive_factor, negative_factor) = match cfg.impact {
        ImpactShape::ZeroFactor => (0, 0),
        _ => (USD / 4_000, USD / 2_000), // 0.025% / 0.05% of imbalance delta
    };
    m.config.position_impact_exponent = USD;
    m.config.position_impact_positive_factor = positive_factor;
    m.config.position_impact_negative_factor = negative_factor;
    m.config.max_positive_position_impact_factor = USD / 50;
    m.config.max_negative_position_impact_factor = USD / 50;
    m.config.max_position_impact_factor_for_liquidations = USD / 50;

    // Open interest. The side being closed must hold at least the position
    // size or the close underflows the pool. Imbalance sets the impact sign:
    // PositiveClamped improves the balance (positive impact, clamped to zero
    // on both paths); Negative worsens it on the same side; Neutral flips the
    // imbalance sign, which the model's cross-over path always counts as a
    // worsening. ZeroFactor keeps the pools neutral and the factors at zero.
    let (long_oi, short_oi) = match (cfg.impact, cfg.is_long) {
        (ImpactShape::Neutral, true) => (SIZE_USD, SIZE_USD / 2),
        (ImpactShape::Neutral, false) => (SIZE_USD / 2, SIZE_USD),
        (ImpactShape::PositiveClamped, true) => (2 * SIZE_USD, SIZE_USD),
        (ImpactShape::PositiveClamped, false) => (SIZE_USD, 2 * SIZE_USD),
        (ImpactShape::Negative, true) => (SIZE_USD, 2 * SIZE_USD),
        (ImpactShape::Negative, false) => (2 * SIZE_USD, SIZE_USD),
        (ImpactShape::ZeroFactor, true) => (SIZE_USD, 0),
        (ImpactShape::ZeroFactor, false) => (0, SIZE_USD),
    };
    m.state.pools.open_interest_for_long.pool.long_token_amount = long_oi;
    m.state.pools.open_interest_in_tokens_for_long
        .pool
        .long_token_amount = long_oi / USD * TOKEN / 20;
    m.state.pools.open_interest_for_short.pool.short_token_amount = short_oi;
    m.state.pools.open_interest_in_tokens_for_short
        .pool
        .short_token_amount = short_oi / USD * TOKEN / 20;

    // Borrowing fee: market cumulative factor ahead of the position checkpoint
    // (zeroed) by the chosen factor.
    let borrowing_factor = USD * cfg.borrowing_fee_bps / 10_000;
    if cfg.is_long {
        m.state.pools.borrowing_factor.pool.long_token_amount = borrowing_factor;
    } else {
        m.state.pools.borrowing_factor.pool.short_token_amount = borrowing_factor;
    }

    // Funding fee: market amount-per-size ahead of the position checkpoint
    // (zeroed); the delta lands in the collateral token.
    let funding_usd = SIZE_USD * cfg.funding_fee_bps / 10_000;
    // funding tokens at 20 USD for the index token, 1 USD for the short token
    let funding_tokens = if cfg.same_token_collateral {
        funding_usd / 20 / (USD / TOKEN)
    } else {
        funding_usd / (USD / TOKEN)
    };
    let funding_delta =
        funding_tokens * (constants::FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT * USD / SIZE_USD);
    let funding_pool = if cfg.is_long {
        &mut m.state.pools.funding_amount_per_size_for_long
    } else {
        &mut m.state.pools.funding_amount_per_size_for_short
    };
    if cfg.same_token_collateral {
        funding_pool.pool.long_token_amount = funding_delta;
    } else {
        funding_pool.pool.short_token_amount = funding_delta;
    }

    let market = MarketModel::from_parts(Arc::new(m), 0);

    // Collateral: sized against the threshold so the position starts healthy
    // but liquidates inside the search range. The fee buffer keeps
    // `remaining_collateral_usd` positive: with collateral barely above the
    // threshold and all three fees high, the subtraction chain would bottom
    // out and the reported price would be None for a boring reason.
    let fees_usd = SIZE_USD * (cfg.order_fee_bps + cfg.borrowing_fee_bps + cfg.funding_fee_bps)
        / 10_000;
    let collateral_usd = match cfg.none_shape {
        NoneShape::DeepUnderwater => {
            liquidation_collateral_usd * cfg.collateral_pct_of_threshold / 100
        }
        _ => {
            liquidation_collateral_usd * cfg.collateral_pct_of_threshold / 100
                + 2 * fees_usd
                + 10 * USD
        }
    };
    let collateral_amount = if cfg.same_token_collateral {
        collateral_usd / 20 / (USD / TOKEN) // index token at spot 20 USD
    } else {
        collateral_usd / (USD / TOKEN) // short token at 1 USD
    };

    let mut pos = Position::zeroed();
    pos.kind = if cfg.is_long { 1 } else { 2 };
    pos.collateral_token = if cfg.same_token_collateral { index } else { short };
    pos.state.size_in_usd = SIZE_USD;
    pos.state.size_in_tokens = SIZE_TOKENS;
    pos.state.collateral_amount = match cfg.none_shape {
        NoneShape::UnwinnableShort => SIZE_TOKENS + funding_tokens + 50 * TOKEN,
        _ => collateral_amount,
    };

    let model = PositionModel::new(market, Arc::new(pos))
        .unwrap_or_else(|e| panic!("{cfg}: fixture failed to build: {e}"));
    Fixture {
        model,
        liquidation_collateral_usd,
        floor_engaged,
    }
}

/// Prices at a given index price with the case's oracle spread. The long
/// token IS the index token in these fixtures, so it moves with the index;
/// the short token stays at 1 USD.
fn prices_at(index_price: u128, spread_bps: u128) -> Prices<u128> {
    let half = index_price * spread_bps / 20_000;
    let px = Price {
        min: index_price - half,
        max: index_price + half,
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
}

/// What the contract's liquidation path says at this price.
fn liquidatable(
    model: &gmsol_sdk::programs::model::PositionModel,
    price: u128,
    spread_bps: u128,
) -> bool {
    model
        .check_liquidatable(&prices_at(price, spread_bps), true, true)
        .map(|r| r.is_some())
        .unwrap_or(false)
}

/// Binary search the real boundary: for a long, the lowest healthy price; for
/// a short, the highest healthy price. Returns None if the endpoints do not
/// bracket a boundary (healthy or liquidatable at both).
fn find_boundary(
    model: &gmsol_sdk::programs::model::PositionModel,
    is_long: bool,
    spread_bps: u128,
) -> Option<u128> {
    let boundary_range = if is_long {
        (PRICE, SPOT) // lo liquidatable, hi healthy
    } else {
        (SPOT, 400 * PRICE) // lo healthy, hi liquidatable
    };
    let (mut lo, mut hi) = boundary_range;
    let healthy = |p: u128| !liquidatable(model, p, spread_bps);
    if is_long {
        if healthy(lo) || !healthy(hi) {
            return None;
        }
    } else if healthy(hi) || !healthy(lo) {
        return None;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if is_long {
            if healthy(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        } else if healthy(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // boundary = the first healthy price for a long, the last healthy for a short
    Some(if is_long { hi } else { lo })
}

/// Run one case, update the ledger, return the relative error in bps when the
/// case is a priced one.
fn run_case(cfg: &CaseParams, ledger: &mut Ledger) -> Option<u128> {
    let fixture = build_fixture(cfg);
    let pm = &fixture.model;

    ledger.hit(if cfg.is_long { "side/long" } else { "side/short" });
    ledger.hit(if cfg.same_token_collateral {
        "collateral/same-token"
    } else {
        "collateral/different-token"
    });
    ledger.hit(if cfg.spread_bps == 0 {
        "spread/zero"
    } else {
        "spread/nonzero"
    });
    if cfg.order_fee_bps > 0 {
        ledger.hit("fee/order");
    }
    if cfg.borrowing_fee_bps > 0 {
        ledger.hit("fee/borrowing");
    }
    if cfg.funding_fee_bps > 0 {
        ledger.hit("fee/funding");
    }
    ledger.hit(if fixture.floor_engaged {
        "floor/engaged"
    } else {
        "floor/clear"
    });

    // Measure the impact the model itself computes at spot, rather than
    // trusting the config to have produced it: the recorded shape is what was
    // actually reached.
    let size_delta = SIZE_USD.to_opposite_signed().expect("signed size delta");
    let impact = pm
        .position_price_impact(&size_delta, false)
        .expect("price impact")
        .value;
    if std::env::var("SWEEP_DEBUG").is_ok() {
        println!("{} impact_value={impact}", cfg.label);
    }
    if impact < 0 {
        ledger.hit("impact/negative");
    } else if impact > 0 {
        ledger.hit("impact/positive-clamped");
    } else {
        ledger.hit("impact/zero");
    }

    let spot_prices = prices_at(SPOT, cfg.spread_bps);
    let reported = pm
        .status(&spot_prices)
        .unwrap_or_else(|e| panic!("{cfg}: status failed: {e}"))
        .liquidation_price;
    let liquidatable_at_spot = liquidatable(pm, SPOT, cfg.spread_bps);

    match cfg.none_shape {
        NoneShape::UnwinnableShort => {
            assert!(!cfg.is_long && cfg.same_token_collateral);
            assert_eq!(
                reported, None,
                "{cfg}: denominator underflow must report no liquidation price"
            );
            // Honest None: the position never liquidates on the way up.
            for &p in &[SPOT, 2 * SPOT, 4 * SPOT, 8 * SPOT] {
                assert!(
                    !liquidatable(pm, p, cfg.spread_bps),
                    "{cfg}: unwinnable short liquidated at {p}, the None was a lie"
                );
            }
            ledger.hit("outcome/honest-none-unwinnable-short");
            return None;
        }
        NoneShape::DeepUnderwater => {
            assert!(
                liquidatable_at_spot,
                "{cfg}: deep-underwater fixture must be liquidatable at spot"
            );
            if let Some(p) = reported {
                // A reported price must agree with "already past the boundary":
                // at or above spot for a long, at or below spot for a short.
                if cfg.is_long {
                    assert!(
                        p >= SPOT * 99 / 100,
                        "{cfg}: reported {p} below spot while already liquidatable"
                    );
                } else {
                    assert!(
                        p <= SPOT * 101 / 100,
                        "{cfg}: reported {p} above spot while already liquidatable"
                    );
                }
            }
            ledger.hit("outcome/already-liquidatable-at-spot");
            return None;
        }
        NoneShape::Priced => {}
    }

    assert!(
        !liquidatable_at_spot,
        "{cfg}: priced fixture must start healthy at spot"
    );
    let reported = reported.unwrap_or_else(|| panic!("{cfg}: expected a liquidation price"));

    let boundary = find_boundary(pm, cfg.is_long, cfg.spread_bps)
        .unwrap_or_else(|| panic!("{cfg}: no liquidation boundary found in range"));

    // Positive control: the oracle must flip around the boundary it produced.
    if cfg.is_long {
        assert!(
            liquidatable(pm, boundary - 1, cfg.spread_bps),
            "{cfg}: oracle stuck healthy below the boundary"
        );
        assert!(
            !liquidatable(pm, boundary, cfg.spread_bps),
            "{cfg}: oracle stuck liquidatable at the boundary"
        );
    } else {
        assert!(
            !liquidatable(pm, boundary, cfg.spread_bps),
            "{cfg}: oracle stuck liquidatable at the boundary"
        );
        assert!(
            liquidatable(pm, boundary + 1, cfg.spread_bps),
            "{cfg}: oracle stuck healthy above the boundary"
        );
    }

    let diff = reported.abs_diff(boundary);
    let bps = diff * 10_000 / boundary.max(1);
    if cfg.spread_bps == 0 {
        // At zero oracle spread the reported price must BE the boundary, up to
        // integer division slack.
        let slack = 16 + boundary / 5_000;
        assert!(
            diff <= slack,
            "{cfg}: reported {reported} vs boundary {boundary} (diff {diff}, {bps}bps)"
        );
    } else {
        // With a spread the single reported price cannot match a boundary the
        // model reads at .min/.max; the half-spread is the inherent budget.
        assert!(
            bps <= cfg.spread_bps / 2 + 5,
            "{cfg}: {bps}bps exceeds the half-spread budget at spread {}bps",
            cfg.spread_bps
        );
    }
    ledger.hit("outcome/priced-consistent");
    Some(bps)
}

/// Build the stratified case list: every (side, collateral, spread, impact)
/// combination gets `reps` randomized instances, so every shape is guaranteed
/// coverage by construction and the ledger assertions catch any future
/// generator change that quietly drops one.
fn cases(seed: u64, reps: usize) -> Vec<CaseParams> {
    let mut rng = Rng::seeded(seed);
    let mut out = Vec::new();
    let mut n = 0usize;
    for &is_long in &[true, false] {
        for &same_token in &[true, false] {
            for &spread_bps in &[0u128, 50] {
                for &impact in &[
                    ImpactShape::Neutral,
                    ImpactShape::PositiveClamped,
                    ImpactShape::Negative,
                    ImpactShape::ZeroFactor,
                ] {
                    for _ in 0..reps {
                        n += 1;
                        out.push(CaseParams {
                            label: format!("case-{n}"),
                            is_long,
                            same_token_collateral: same_token,
                            spread_bps,
                            order_fee_bps: rng.below(26) as u128,
                            borrowing_fee_bps: rng.below(41) as u128,
                            funding_fee_bps: rng.below(21) as u128,
                            impact,
                            floor_pct: if rng.coin() {
                                100 + rng.below(61) as u128
                            } else {
                                0
                            },
                            collateral_pct_of_threshold: 105 + rng.below(700) as u128,
                            none_shape: NoneShape::Priced,
                        });
                    }
                }
            }
        }
    }
    // Dedicated None shapes.
    for i in 0..4 {
        let is_long = i % 2 == 0;
        for &same_token in &[true, false] {
            out.push(CaseParams {
                label: format!("underwater-{i}-{same_token}"),
                is_long,
                same_token_collateral: same_token,
                spread_bps: 0,
                order_fee_bps: 10,
                borrowing_fee_bps: 10,
                funding_fee_bps: 5,
                impact: ImpactShape::Neutral,
                floor_pct: 0,
                collateral_pct_of_threshold: 20,
                none_shape: NoneShape::DeepUnderwater,
            });
        }
    }
    for i in 0..4 {
        out.push(CaseParams {
            label: format!("unwinnable-{i}"),
            is_long: false,
            same_token_collateral: true,
            spread_bps: 0,
            order_fee_bps: i as u128 * 5,
            borrowing_fee_bps: 10,
            funding_fee_bps: 5,
            impact: ImpactShape::Neutral,
            floor_pct: 0,
            collateral_pct_of_threshold: 300,
            none_shape: NoneShape::UnwinnableShort,
        });
    }
    out
}

#[test]
fn sweep_liquidation_price_vs_contract_condition() {
    let cases = cases(20_260_915, 5);
    let mut ledger = Ledger::default();
    let mut worst_bps = 0u128;
    let mut worst_label = String::new();
    let mut priced = 0usize;
    for cfg in &cases {
        if let Some(bps) = run_case(cfg, &mut ledger) {
            priced += 1;
            if bps > worst_bps {
                worst_bps = bps;
                worst_label = cfg.label.clone();
            }
        }
    }

    println!("sweep: {} cases, {} priced, worst error {worst_bps}bps ({worst_label})",
        cases.len(), priced);
    for (shape, count) in &ledger.0 {
        println!("  {shape}: {count}");
    }

    ledger.assert_coverage(&[
        ("side/long", 30),
        ("side/short", 30),
        ("collateral/same-token", 30),
        ("collateral/different-token", 30),
        ("spread/zero", 30),
        ("spread/nonzero", 30),
        ("impact/negative", 20),
        ("impact/positive-clamped", 20),
        ("impact/zero", 20),
        ("fee/order", 40),
        ("fee/borrowing", 40),
        ("fee/funding", 40),
        ("floor/engaged", 20),
        ("floor/clear", 20),
        ("outcome/priced-consistent", 100),
        ("outcome/already-liquidatable-at-spot", 8),
        ("outcome/honest-none-unwinnable-short", 4),
    ]);
}
