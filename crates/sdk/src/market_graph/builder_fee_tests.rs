//! Order simulations with a builder fee, against the market graph fixture.
//!
//! Expected values are computed here from the formulas the program documents,
//! not by calling the code under test. The scenarios that depend on the
//! fixture's market parameters (leverage and minimum collateral limits) say what
//! they rely on.

use std::sync::Arc;

use gmsol_model::{price::Price, PerpMarketMut, PoolExt};
use gmsol_programs::{constants::MARKET_USD_UNIT, gmsol_store::accounts::Position};
use solana_sdk::pubkey::Pubkey;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams, DecreasePositionSwapType},
    simulation::{order::OrderSimulationOutput, SimulationOptions, Simulator},
};

use super::tests::{create_and_update_market_graph, SOL_BALANCED_MARKET_TOKEN, USDC, WSOL};

/// One percent.
const ONE_PERCENT: u128 = MARKET_USD_UNIT / 100;
/// Fifty percent: far above any cap the program allows, which the simulator does
/// not enforce. Used to make the fee large enough to exceed the funds.
const HALF: u128 = MARKET_USD_UNIT / 2;

struct Fixture {
    simulator: Simulator,
    market_token: Pubkey,
    index_token: Pubkey,
    wsol: Pubkey,
    usdc: Pubkey,
}

fn fixture() -> Fixture {
    let (graph, _) = create_and_update_market_graph().unwrap();
    let simulator = graph.to_simulator(Default::default());
    let market_token: Pubkey = SOL_BALANCED_MARKET_TOKEN.parse().unwrap();
    let index_token = simulator
        .get_market(&market_token)
        .unwrap()
        .meta
        .index_token_mint;
    Fixture {
        simulator,
        market_token,
        index_token,
        wsol: WSOL.parse().unwrap(),
        usdc: USDC.parse().unwrap(),
    }
}

impl Fixture {
    fn price(&self, token: &Pubkey) -> Price<u128> {
        self.simulator.get_price(token).unwrap()
    }

    /// Moves the index price by `bps` basis points (negative to drop it) on
    /// `simulator`, leaving the collateral token prices alone.
    fn move_index_price(&self, simulator: &mut Simulator, bps: i128) {
        let base = self.price(&self.index_token);
        let scale = |price: u128| -> u128 {
            u128::try_from(i128::try_from(price).unwrap() * (10_000 + bps) / 10_000).unwrap()
        };
        simulator
            .insert_price(
                &self.index_token,
                Arc::new(Price {
                    min: scale(base.min),
                    max: scale(base.max),
                }),
            )
            .unwrap();
    }

    /// Opens a long position without a builder fee and returns it.
    fn open_long(
        &mut self,
        collateral_token: &Pubkey,
        amount: u128,
        size_usd: u128,
    ) -> Arc<Position> {
        let params = CreateOrderParams::builder()
            .market_token(self.market_token)
            .is_long(true)
            .size(size_usd * MARKET_USD_UNIT)
            .amount(amount)
            .build();
        let output = self
            .simulator
            .simulate_order(CreateOrderKind::MarketIncrease, &params, collateral_token)
            .build()
            .execute_with_options(Default::default())
            .unwrap();
        let OrderSimulationOutput::Increase { position, .. } = output else {
            unreachable!()
        };
        position.position_arc().clone()
    }

    fn decrease_params(&self, size_usd: u128, amount: u128) -> CreateOrderParams {
        CreateOrderParams::builder()
            .market_token(self.market_token)
            .is_long(true)
            .size(size_usd * MARKET_USD_UNIT)
            .amount(amount)
            .build()
    }
}

/// The program's fee for a whole-dollar size: `ceil(size * factor / price.min)`.
fn expected_fee(size_usd: u128, factor: u128, price: &Price<u128>) -> u128 {
    // The size is `size_usd * UNIT` in the model's fixed point, so
    // `size * factor / UNIT` is exactly `size_usd * factor`.
    (size_usd * factor).div_ceil(price.min)
}

fn market_state(simulator: &Simulator, market_token: &Pubkey) -> String {
    format!("{:?}", simulator.get_market(market_token).unwrap())
}

fn error_of(result: crate::Result<OrderSimulationOutput>) -> String {
    result.expect_err("the simulation must fail").to_string()
}

/// Runs one order simulation.
#[allow(clippy::too_many_arguments)]
fn run(
    simulator: &mut Simulator,
    kind: CreateOrderKind,
    params: &CreateOrderParams,
    token: &Pubkey,
    pay_token: Option<&Pubkey>,
    receive_token: Option<&Pubkey>,
    swap_path: &[Pubkey],
    position: Option<&Arc<Position>>,
    builder_fee_factor: u128,
) -> crate::Result<OrderSimulationOutput> {
    simulator
        .simulate_order(kind, params, token)
        .pay_token(pay_token)
        .receive_token(receive_token)
        .swap_path(swap_path)
        .position(position)
        .builder_fee_factor(builder_fee_factor)
        .build()
        .execute_with_options(SimulationOptions {
            skip_limit_price_validation: true,
            disable_vis: false,
        })
}

fn market_decrease(
    simulator: &mut Simulator,
    params: &CreateOrderParams,
    token: &Pubkey,
    position: &Arc<Position>,
    factor: u128,
) -> crate::Result<OrderSimulationOutput> {
    run(
        simulator,
        CreateOrderKind::MarketDecrease,
        params,
        token,
        None,
        None,
        &[],
        Some(position),
        factor,
    )
}

/// The receipts add up: what the owner holds plus the fee is what was produced,
/// and the fee on the receipts is the fee that was charged.
fn assert_consistent(output: &OrderSimulationOutput) {
    let OrderSimulationOutput::Decrease {
        receipts,
        builder_fee,
        ..
    } = output
    else {
        panic!("not a decrease output");
    };
    let gross: u128 = receipts.iter().map(|r| r.gross_amount).sum();
    let net: u128 = receipts.iter().map(|r| r.net_amount).sum();
    let fee: u128 = receipts.iter().map(|r| u128::from(r.fee_amount)).sum();
    assert_eq!(net + fee, gross);
    assert_eq!(
        fee,
        builder_fee
            .as_ref()
            .map_or(0, |fee| u128::from(fee.paid_amount))
    );
    if let Some(fee) = builder_fee {
        assert!(u128::from(fee.paid_amount) <= fee.payable_amount);
        assert_eq!(
            receipts[0].token, fee.token,
            "the fee is on the final token"
        );
    }
    for receipt in receipts {
        assert!(receipt.gross_amount > 0 || receipt.token == receipts[0].token);
    }
}

// ----------------------------------------------------------------------------
// Increase
// ----------------------------------------------------------------------------

#[test]
fn increase_deducts_the_fee_before_the_position_grows() {
    let f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let wsol_price = f.price(&wsol);
    let gross = 1_000_000_000u128;
    let fee = expected_fee(500, ONE_PERCENT, &wsol_price);
    assert_eq!(
        fee, 46_205_482,
        "hand-checked: $5 at the WSOL minimum price"
    );

    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(gross)
        .build();

    let mut with_fee = f.simulator.clone();
    let output = run(
        &mut with_fee,
        CreateOrderKind::MarketIncrease,
        &params,
        &wsol,
        None,
        None,
        &[],
        None,
        ONE_PERCENT,
    )
    .unwrap();

    let OrderSimulationOutput::Increase {
        report,
        position,
        gross_collateral_amount,
        builder_fee,
        ..
    } = &output
    else {
        panic!()
    };
    assert_eq!(*gross_collateral_amount, gross);
    let builder_fee = builder_fee.as_ref().unwrap();
    assert_eq!(builder_fee.factor, ONE_PERCENT);
    assert_eq!(builder_fee.token, wsol);
    assert_eq!(builder_fee.payable_amount, fee);
    assert_eq!(u128::from(builder_fee.paid_amount), fee);
    assert_eq!(output.net_collateral_amount(), Some(gross - fee));
    assert_eq!(
        *report.params().collateral_increment_amount(),
        gross - fee,
        "the model must see the net amount, as the program's trade event does"
    );

    // The same position as an order without a fee that was funded with the net
    // amount directly.
    let net_params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(gross - fee)
        .build();
    let mut without_fee = f.simulator.clone();
    let baseline = run(
        &mut without_fee,
        CreateOrderKind::MarketIncrease,
        &net_params,
        &wsol,
        None,
        None,
        &[],
        None,
        0,
    )
    .unwrap();
    let OrderSimulationOutput::Increase {
        position: baseline_position,
        ..
    } = &baseline
    else {
        panic!()
    };
    let (a, b) = (
        &position.position().state,
        &baseline_position.position().state,
    );
    assert_eq!(a.collateral_amount, b.collateral_amount);
    assert_eq!(a.size_in_usd, b.size_in_usd);
    assert_eq!(a.size_in_tokens, b.size_in_tokens);

    // And it is less collateral than the fee-less order with the gross amount.
    let gross_params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(gross)
        .build();
    let mut gross_sim = f.simulator.clone();
    let gross_run = run(
        &mut gross_sim,
        CreateOrderKind::MarketIncrease,
        &gross_params,
        &wsol,
        None,
        None,
        &[],
        None,
        0,
    )
    .unwrap();
    let OrderSimulationOutput::Increase {
        position: gross_position,
        ..
    } = &gross_run
    else {
        panic!()
    };
    assert!(gross_position.position().state.collateral_amount > a.collateral_amount);
}

#[test]
fn increase_takes_the_fee_from_the_swapped_amount() {
    let mut f = fixture();
    let (wsol, usdc, market_token) = (f.wsol, f.usdc, f.market_token);
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(120_000_000) // 120 USDC
        .build();

    let output = run(
        &mut f.simulator,
        CreateOrderKind::MarketIncrease,
        &params,
        &wsol,
        Some(&usdc),
        None,
        &[market_token],
        None,
        ONE_PERCENT,
    )
    .unwrap();
    let OrderSimulationOutput::Increase {
        swaps,
        gross_collateral_amount,
        builder_fee,
        ..
    } = &output
    else {
        panic!()
    };
    assert_eq!(swaps.len(), 1);
    assert_eq!(
        *gross_collateral_amount,
        *swaps[0].token_out_amount(),
        "gross is what the pay-token swap produced, not the amount that was paid"
    );
    let fee = builder_fee.as_ref().unwrap();
    // Denominated in the collateral token and priced with its minimum price.
    assert_eq!(fee.token, wsol);
    assert_eq!(
        fee.payable_amount,
        expected_fee(500, ONE_PERCENT, &f.price(&wsol))
    );
    assert_eq!(
        output.net_collateral_amount(),
        Some(gross_collateral_amount - u128::from(fee.paid_amount))
    );
}

#[test]
fn increase_that_cannot_cover_the_fee_fails_and_leaves_the_simulator_untouched() {
    let mut f = fixture();
    let (wsol, usdc, market_token) = (f.wsol, f.usdc, f.market_token);
    // $2 of collateral on a $20 position. At 50% the fee is $10.
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(20 * MARKET_USD_UNIT)
        .amount(2_000_000)
        .build();
    let before = market_state(&f.simulator, &market_token);

    let err = error_of(run(
        &mut f.simulator,
        CreateOrderKind::MarketIncrease,
        &params,
        &wsol,
        Some(&usdc),
        None,
        &[market_token],
        None,
        HALF,
    ));
    assert!(err.contains("BuilderFeeExceedsCollateral"), "{err}");
    assert_eq!(
        market_state(&f.simulator, &market_token),
        before,
        "the swap that preceded the failure must not stay applied, as on chain"
    );

    // Control: the same order without a fee succeeds and does move the pool,
    // so the assertion above can tell the difference.
    run(
        &mut f.simulator,
        CreateOrderKind::MarketIncrease,
        &params,
        &wsol,
        Some(&usdc),
        None,
        &[market_token],
        None,
        0,
    )
    .unwrap();
    assert_ne!(market_state(&f.simulator, &market_token), before);
}

#[test]
fn increase_fee_boundary() {
    let f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let fee = expected_fee(500, ONE_PERCENT, &f.price(&wsol));

    let at = |amount: u128| {
        let params = CreateOrderParams::builder()
            .market_token(market_token)
            .is_long(true)
            .size(500 * MARKET_USD_UNIT)
            .amount(amount)
            .build();
        run(
            &mut f.simulator.clone(),
            CreateOrderKind::MarketIncrease,
            &params,
            &wsol,
            None,
            None,
            &[],
            None,
            ONE_PERCENT,
        )
    };

    // One unit short of the fee: the order fails for the fee.
    let err = error_of(at(fee - 1));
    assert!(err.contains("BuilderFeeExceedsCollateral"), "{err}");
    // Exactly the fee: the fee is covered. Whatever the model then does with an
    // empty position is its own business, but it is not a fee shortfall.
    match at(fee) {
        Ok(output) => assert_eq!(output.net_collateral_amount(), Some(0)),
        Err(err) => assert!(
            !err.to_string().contains("BuilderFeeExceedsCollateral"),
            "{err}"
        ),
    }
}

#[test]
fn fee_applies_to_limit_increases() {
    let f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let fee = expected_fee(500, ONE_PERCENT, &f.price(&wsol));
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(1_000_000_000)
        .trigger_price(f.price(&f.index_token).max)
        .build();
    let output = run(
        &mut f.simulator.clone(),
        CreateOrderKind::LimitIncrease,
        &params,
        &wsol,
        None,
        None,
        &[],
        None,
        ONE_PERCENT,
    )
    .unwrap();
    assert_eq!(output.net_collateral_amount(), Some(1_000_000_000 - fee));
    assert_eq!(output.builder_fee().unwrap().payable_amount, fee);
}

#[test]
fn zero_factor_increase_has_no_fee_and_gross_equals_net() {
    let mut f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(1_000_000_000)
        .build();
    let output = run(
        &mut f.simulator,
        CreateOrderKind::MarketIncrease,
        &params,
        &wsol,
        None,
        None,
        &[],
        None,
        0,
    )
    .unwrap();
    assert!(output.builder_fee().is_none());
    let OrderSimulationOutput::Increase {
        gross_collateral_amount,
        ..
    } = &output
    else {
        panic!()
    };
    assert_eq!(
        output.net_collateral_amount(),
        Some(*gross_collateral_amount)
    );
}

// ----------------------------------------------------------------------------
// Decrease
//
// The position is a $500 long with 1 SOL (998_151_781 lamports after the order
// fee) as collateral. The fixture's minimum collateral value and minimum position
// size are both $1.
// ----------------------------------------------------------------------------

#[test]
fn decrease_tops_up_the_withdrawal_by_the_fee_estimate() {
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let estimate = expected_fee(100, ONE_PERCENT, &f.price(&wsol));
    assert_eq!(estimate, 9_241_097);

    let params = f.decrease_params(100, 900_000_000);
    let without_fee =
        market_decrease(&mut f.simulator.clone(), &params, &wsol, &position, 0).unwrap();
    let with_fee = market_decrease(
        &mut f.simulator.clone(),
        &params,
        &wsol,
        &position,
        ONE_PERCENT,
    )
    .unwrap();

    let (
        OrderSimulationOutput::Decrease { report: base, .. },
        OrderSimulationOutput::Decrease {
            report,
            builder_fee,
            receipts,
            ..
        },
    ) = (&without_fee, &with_fee)
    else {
        panic!()
    };
    assert_eq!(*base.withdrawable_collateral_amount(), 900_000_000);
    assert_eq!(
        *report.withdrawable_collateral_amount(),
        900_000_000 + estimate,
        "the fee-funded withdrawal must reach the model"
    );
    // Executed size is the requested size, so the charge equals the estimate.
    let fee = builder_fee.as_ref().unwrap();
    assert_eq!(fee.payable_amount, estimate);
    assert_eq!(u128::from(fee.paid_amount), estimate);
    // The report is the gross payout; the receipt takes the fee out of it.
    assert_eq!(*report.output_amount(), 900_000_000 + estimate);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].token, wsol);
    assert_eq!(receipts[0].gross_amount, 900_000_000 + estimate);
    assert_eq!(receipts[0].net_amount, 900_000_000);
    assert_consistent(&with_fee);
    assert_consistent(&without_fee);
}

#[test]
fn decrease_withdraws_collateral_for_the_fee_even_when_none_was_requested() {
    // The position is under water enough that no collateral can be withdrawn
    // without the fee, but the fee-funded part is. All of it goes to the fee.
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let mut sim = f.simulator.clone();
    f.move_index_price(&mut sim, -2090);
    let params = f.decrease_params(100, 0);

    let without_fee = market_decrease(&mut sim.clone(), &params, &wsol, &position, 0).unwrap();
    let with_fee = market_decrease(&mut sim, &params, &wsol, &position, ONE_PERCENT).unwrap();

    assert_eq!(without_fee.receipts()[0].gross_amount, 0);
    let receipt = &with_fee.receipts()[0];
    assert_eq!(receipt.gross_amount, 9_241_097);
    assert_eq!(u128::from(receipt.fee_amount), receipt.gross_amount);
    assert_eq!(
        receipt.net_amount, 0,
        "the owner nets nothing, the builder is paid"
    );
    assert_consistent(&with_fee);
}

#[test]
fn the_fee_top_up_can_make_a_decrease_fail_that_succeeds_without_a_fee() {
    // 970M lamports leave the position just above the leverage limit. The fee
    // top-up takes the remainder below it, and the program fails the order.
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let params = f.decrease_params(100, 970_000_000);

    assert!(market_decrease(&mut f.simulator.clone(), &params, &wsol, &position, 0).is_ok());
    let mut sim = f.simulator.clone();
    let before = market_state(&sim, &f.market_token);
    let err = error_of(market_decrease(
        &mut sim,
        &params,
        &wsol,
        &position,
        ONE_PERCENT,
    ));
    assert!(err.contains("min collateral for leverage"), "{err}");
    assert_eq!(market_state(&sim, &f.market_token), before);
}

#[test]
fn a_top_up_the_position_cannot_afford_is_dropped_and_nothing_is_charged() {
    // 975M lamports: allowed without a fee. With the top-up the remaining
    // collateral would be insufficient, so the model drops the withdrawal
    // altogether. No output funds the fee: it is still computed, and charges 0.
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let params = f.decrease_params(100, 975_000_000);

    let without_fee =
        market_decrease(&mut f.simulator.clone(), &params, &wsol, &position, 0).unwrap();
    assert_eq!(without_fee.receipts()[0].gross_amount, 975_000_000);

    let with_fee = market_decrease(
        &mut f.simulator.clone(),
        &params,
        &wsol,
        &position,
        ONE_PERCENT,
    )
    .unwrap();
    let fee = with_fee.builder_fee().expect("the order has a fee factor");
    assert_eq!(fee.payable_amount, 9_241_097);
    assert_eq!(fee.paid_amount, 0);
    assert_eq!(with_fee.receipts()[0].gross_amount, 0);
    assert_eq!(with_fee.receipts()[0].net_amount, 0);
    assert_consistent(&with_fee);
}

#[test]
fn a_fee_can_turn_a_failing_decrease_into_a_full_close_charged_on_the_executed_size() {
    // With the index price 21.26% down, the $100 decrease fails without a fee
    // ("min collateral for leverage"). With one, the top-up leaves too little
    // for a partial decrease and the model closes the whole position, which is
    // allowed.
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let mut sim = f.simulator.clone();
    f.move_index_price(&mut sim, -2126);
    let params = f.decrease_params(100, 0);

    let err = error_of(market_decrease(
        &mut sim.clone(),
        &params,
        &wsol,
        &position,
        0,
    ));
    assert!(err.contains("min collateral for leverage"), "{err}");

    let output = market_decrease(&mut sim, &params, &wsol, &position, ONE_PERCENT).unwrap();
    let OrderSimulationOutput::Decrease {
        report,
        builder_fee,
        receipts,
        ..
    } = &output
    else {
        panic!()
    };
    assert!(report.should_remove());
    assert_eq!(
        *report.size_delta_usd(),
        500 * MARKET_USD_UNIT,
        "a full close"
    );

    let fee = builder_fee.as_ref().unwrap();
    let estimate = expected_fee(100, ONE_PERCENT, &f.price(&wsol));
    // Charged on the executed $500, not the requested $100 the estimate used.
    assert_eq!(
        fee.payable_amount,
        expected_fee(500, ONE_PERCENT, &f.price(&wsol))
    );
    assert!(fee.payable_amount > estimate);
    // The position closes at a loss and has less than the fee left: the fee is
    // clamped to the whole output, and the owner receives nothing.
    assert_eq!(u128::from(fee.paid_amount), *report.output_amount());
    assert!(u128::from(fee.paid_amount) < fee.payable_amount);
    assert_eq!(receipts[0].net_amount, 0);
    assert_consistent(&output);
}

#[test]
fn decrease_fee_is_clamped_to_the_output() {
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    // 50% of $500 is $250, more than the roughly $108 position.
    let params = f.decrease_params(500, 0);
    let output = market_decrease(&mut f.simulator, &params, &wsol, &position, HALF).unwrap();

    let fee = output.builder_fee().unwrap();
    assert_eq!(fee.payable_amount, expected_fee(500, HALF, &f.price(&wsol)));
    let receipt = &output.receipts()[0];
    assert!(u128::from(fee.paid_amount) < fee.payable_amount);
    assert_eq!(u128::from(fee.paid_amount), receipt.gross_amount);
    assert_eq!(receipt.net_amount, 0);
    assert_consistent(&output);
}

#[test]
fn zero_factor_decrease_has_no_fee_and_gross_equals_net() {
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let params = f.decrease_params(100, 500_000_000);
    let output = market_decrease(&mut f.simulator, &params, &wsol, &position, 0).unwrap();
    assert!(output.builder_fee().is_none());
    let receipt = &output.receipts()[0];
    assert_eq!(receipt.gross_amount, 500_000_000);
    assert_eq!(receipt.fee_amount, 0);
    assert_eq!(receipt.net_amount, 500_000_000);
}

#[test]
fn fee_applies_to_limit_and_stop_loss_decreases() {
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let mut expected = None;
    for kind in [
        CreateOrderKind::MarketDecrease,
        CreateOrderKind::LimitDecrease,
        CreateOrderKind::StopLossDecrease,
    ] {
        let params = CreateOrderParams::builder()
            .market_token(f.market_token)
            .is_long(true)
            .size(100 * MARKET_USD_UNIT)
            .amount(500_000_000)
            .trigger_price(f.price(&f.index_token).min)
            .build();
        let output = run(
            &mut f.simulator.clone(),
            kind,
            &params,
            &wsol,
            None,
            None,
            &[],
            Some(&position),
            ONE_PERCENT,
        )
        .unwrap();
        let fee = output.builder_fee().cloned().unwrap();
        assert_consistent(&output);
        assert_eq!(*expected.get_or_insert(fee.clone()), fee);
    }
}

// ----------------------------------------------------------------------------
// Decrease with a secondary (pnl token) output
//
// A long on USDC collateral: profit is paid in WSOL, so the position has two
// outputs in two tokens.
// ----------------------------------------------------------------------------

struct Profitable {
    f: Fixture,
    simulator: Simulator,
    position: Arc<Position>,
}

fn profitable_usdc_long() -> Profitable {
    let mut f = fixture();
    let usdc = f.usdc;
    let position = f.open_long(&usdc, 200_000_000, 500); // 200 USDC
    let mut simulator = f.simulator.clone();
    f.move_index_price(&mut simulator, 1000); // +10%
    Profitable {
        f,
        simulator,
        position,
    }
}

#[test]
fn decrease_with_a_different_token_secondary_charges_the_final_token_only() {
    let p = profitable_usdc_long();
    let (usdc, wsol) = (p.f.usdc, p.f.wsol);
    let params = p.f.decrease_params(500, 0);
    let output = market_decrease(
        &mut p.simulator.clone(),
        &params,
        &usdc,
        &p.position,
        ONE_PERCENT,
    )
    .unwrap();

    let OrderSimulationOutput::Decrease {
        report, receipts, ..
    } = &output
    else {
        panic!()
    };
    assert!(
        *report.secondary_output_amount() > 0,
        "the profit is paid in WSOL"
    );

    // Fee in USDC, at the USDC price (not the WSOL price).
    let fee = output.builder_fee().unwrap();
    assert_eq!(fee.token, usdc);
    assert_eq!(
        fee.payable_amount,
        expected_fee(500, ONE_PERCENT, &p.f.price(&usdc))
    );
    assert_eq!(
        u128::from(fee.paid_amount),
        fee.payable_amount,
        "200 USDC covers $5"
    );

    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].token, usdc);
    assert_eq!(receipts[0].gross_amount, *report.output_amount());
    assert_eq!(receipts[0].fee_amount, fee.paid_amount);
    assert_eq!(receipts[1].token, wsol);
    assert_eq!(receipts[1].gross_amount, *report.secondary_output_amount());
    assert_eq!(
        receipts[1].fee_amount, 0,
        "the WSOL profit does not pay a USDC fee"
    );
    assert_consistent(&output);
}

#[test]
fn decrease_with_a_same_token_secondary_merges_the_outputs_and_prices_the_final_token() {
    let p = profitable_usdc_long();
    let (usdc, wsol, market_token) = (p.f.usdc, p.f.wsol, p.f.market_token);
    let params = p.f.decrease_params(500, 0);
    // Receive WSOL: the USDC collateral is swapped into it, and the WSOL profit
    // joins it in the same escrow.
    let output = run(
        &mut p.simulator.clone(),
        CreateOrderKind::MarketDecrease,
        &params,
        &usdc,
        None,
        Some(&wsol),
        &[market_token],
        Some(&p.position),
        ONE_PERCENT,
    )
    .unwrap();

    let OrderSimulationOutput::Decrease {
        swaps,
        report,
        receipts,
        ..
    } = &output
    else {
        panic!()
    };
    assert_eq!(swaps.len(), 1);
    let swapped = *swaps[0].token_out_amount();
    let profit = *report.secondary_output_amount();
    assert!(profit > 0);

    assert_eq!(receipts.len(), 1, "one escrow, one receipt");
    assert_eq!(receipts[0].token, wsol);
    assert_eq!(receipts[0].gross_amount, swapped + profit);

    // Priced in the final output token, not in the USDC collateral.
    let fee = output.builder_fee().unwrap();
    assert_eq!(fee.token, wsol);
    assert_eq!(
        fee.payable_amount,
        expected_fee(500, ONE_PERCENT, &p.f.price(&wsol))
    );
    assert_ne!(
        fee.payable_amount,
        expected_fee(500, ONE_PERCENT, &p.f.price(&usdc))
    );
    assert_eq!(
        receipts[0].net_amount,
        swapped + profit - u128::from(fee.paid_amount)
    );
    assert_consistent(&output);
}

#[test]
fn collateral_to_pnl_token_is_rejected_with_a_fee_and_allowed_without() {
    let p = profitable_usdc_long();
    let (usdc, market_token) = (p.f.usdc, p.f.market_token);
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(500 * MARKET_USD_UNIT)
        .amount(0)
        .decrease_position_swap_type(DecreasePositionSwapType::CollateralToPnlToken)
        .build();

    let mut sim = p.simulator.clone();
    let before = market_state(&sim, &market_token);
    let err = error_of(market_decrease(
        &mut sim,
        &params,
        &usdc,
        &p.position,
        ONE_PERCENT,
    ));
    assert!(err.contains("BuilderFeeSwapTypeNotAllowed"), "{err}");
    assert_eq!(
        market_state(&sim, &market_token),
        before,
        "rejected before the market moved"
    );

    // The rejection is keyed off the factor, not on the estimate: a dust size
    // that rounds the estimate to zero is rejected too.
    let dust = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(1)
        .amount(0)
        .decrease_position_swap_type(DecreasePositionSwapType::CollateralToPnlToken)
        .build();
    let err = error_of(market_decrease(
        &mut sim,
        &dust,
        &usdc,
        &p.position,
        ONE_PERCENT,
    ));
    assert!(err.contains("BuilderFeeSwapTypeNotAllowed"), "{err}");

    market_decrease(&mut sim, &params, &usdc, &p.position, 0).unwrap();
}

// ----------------------------------------------------------------------------
// Funding rebates
//
// Besides the trade outputs, an execution pays out the funding rebates the
// position is owed, in the market's long and short tokens
// (`report.claimable_funding_amounts()`). The receipts must count them, merged
// by token with the outputs, while the builder fee cap must not: the program
// clamps the fee to the final output and the same-token secondary output only.
// ----------------------------------------------------------------------------

/// Per-size deltas that leave the $500 positions of these tests owed a rebate in
/// both the long and the short token (`size * delta / (adjustment * UNIT)`).
const LONG_FUNDING_DELTA: i128 = 20_000_000_000_000;
const SHORT_FUNDING_DELTA: i128 = 10_000_000_000_000;

impl Fixture {
    /// Makes the market owe the positions of its long side a funding rebate.
    ///
    /// Positions opened earlier hold the market's claimable funding amount per
    /// size as of their opening. Raising the market's amount by `long_delta` /
    /// `short_delta` leaves them owed the difference, in the long / short token.
    fn add_funding_rebates(&self, simulator: &mut Simulator, long_delta: i128, short_delta: i128) {
        let market = simulator.get_market_mut(&self.market_token).unwrap();
        let pool = market
            .claimable_funding_amount_per_size_pool_mut(true)
            .unwrap();
        pool.apply_delta_amount(true, &long_delta).unwrap();
        pool.apply_delta_amount(false, &short_delta).unwrap();
    }
}

/// The long and short token funding rebates of a decrease, from its report.
fn rebates(output: &OrderSimulationOutput) -> (u128, u128) {
    let OrderSimulationOutput::Decrease { report, .. } = output else {
        panic!("not a decrease output");
    };
    let (long, short) = report.claimable_funding_amounts();
    (*long, *short)
}

#[test]
fn receipts_include_the_funding_rebates() {
    let mut f = fixture();
    let (wsol, usdc) = (f.wsol, f.usdc);
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let params = f.decrease_params(100, 500_000_000);

    // Nothing is owed: the receipts hold the output alone.
    let mut plain = f.simulator.clone();
    let without_rebates = market_decrease(&mut plain, &params, &wsol, &position, 0).unwrap();
    assert_eq!(rebates(&without_rebates), (0, 0));
    assert_eq!(without_rebates.receipts().len(), 1);

    let mut owed = f.simulator.clone();
    f.add_funding_rebates(&mut owed, LONG_FUNDING_DELTA, SHORT_FUNDING_DELTA);
    let output = market_decrease(&mut owed, &params, &wsol, &position, 0).unwrap();
    let (long, short) = rebates(&output);
    assert!(
        long > 0 && short > 0,
        "the position is owed a rebate in both tokens"
    );

    let OrderSimulationOutput::Decrease { report, .. } = &output else {
        panic!("not a decrease output");
    };
    // WSOL is the collateral, the final output and the long token, so its
    // receipt is the output plus the long rebate. The short rebate is USDC, a
    // token the position pays nothing else in: it must not go missing.
    let receipts = output.receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].token, wsol);
    assert_eq!(receipts[0].gross_amount, *report.output_amount() + long);
    assert_eq!(receipts[0].fee_amount, 0);
    assert_eq!(receipts[0].net_amount, receipts[0].gross_amount);
    assert_eq!(receipts[1].token, usdc);
    assert_eq!(receipts[1].gross_amount, short);
    assert_eq!(receipts[1].net_amount, short);
    assert!(output.builder_fee().is_none());
    assert_consistent(&output);
}

#[test]
fn funding_rebates_do_not_widen_the_builder_fee_cap() {
    let mut f = fixture();
    let wsol = f.wsol;
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    // 50% of $500 is $250, more than the whole position is worth.
    let params = f.decrease_params(500, 0);

    let mut owed = f.simulator.clone();
    f.add_funding_rebates(&mut owed, LONG_FUNDING_DELTA, SHORT_FUNDING_DELTA);
    let output = market_decrease(&mut owed, &params, &wsol, &position, HALF).unwrap();
    let (long, _) = rebates(&output);
    assert!(long > 0, "the position is owed a long token rebate");

    let OrderSimulationOutput::Decrease { report, .. } = &output else {
        panic!("not a decrease output");
    };
    let fee = output.builder_fee().unwrap();
    assert_eq!(fee.payable_amount, expected_fee(500, HALF, &f.price(&wsol)));
    assert!(u128::from(fee.paid_amount) < fee.payable_amount);
    assert_eq!(
        u128::from(fee.paid_amount),
        *report.output_amount(),
        "the fee is clamped to the output, and the rebate does not fund it"
    );

    // The rebate is in the same WSOL escrow, and is what the owner keeps.
    let receipt = &output.receipts()[0];
    assert_eq!(receipt.token, wsol);
    assert_eq!(receipt.gross_amount, *report.output_amount() + long);
    assert_eq!(receipt.fee_amount, fee.paid_amount);
    assert_eq!(receipt.net_amount, long);
    assert_consistent(&output);
}

#[test]
fn funding_rebates_merge_with_the_outputs_by_token() {
    let p = profitable_usdc_long();
    let (usdc, wsol) = (p.f.usdc, p.f.wsol);
    let params = p.f.decrease_params(500, 0);

    let mut plain = p.simulator.clone();
    let without_rebates =
        market_decrease(&mut plain, &params, &usdc, &p.position, ONE_PERCENT).unwrap();

    let mut owed = p.simulator.clone();
    p.f.add_funding_rebates(&mut owed, LONG_FUNDING_DELTA, SHORT_FUNDING_DELTA);
    let output = market_decrease(&mut owed, &params, &usdc, &p.position, ONE_PERCENT).unwrap();
    let (long, short) = rebates(&output);
    assert!(
        long > 0 && short > 0,
        "the position is owed a rebate in both tokens"
    );

    let OrderSimulationOutput::Decrease { report, .. } = &output else {
        panic!("not a decrease output");
    };
    let profit = *report.secondary_output_amount();
    assert!(profit > 0, "the profit is paid in WSOL");

    // USDC: the final output plus the short rebate, which carries the fee.
    // WSOL: the pnl output plus the long rebate.
    let receipts = output.receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].token, usdc);
    assert_eq!(receipts[0].gross_amount, *report.output_amount() + short);
    assert_eq!(receipts[1].token, wsol);
    assert_eq!(receipts[1].gross_amount, profit + long);
    assert_eq!(receipts[1].fee_amount, 0);

    // The fee is the one the same order is charged without any rebate.
    let fee = output.builder_fee().unwrap();
    assert_eq!(output.builder_fee(), without_rebates.builder_fee());
    assert_eq!(receipts[0].fee_amount, fee.paid_amount);
    assert_consistent(&output);
}

#[test]
fn funding_rebates_join_a_receive_token_reached_through_a_swap() {
    let p = profitable_usdc_long();
    let (usdc, wsol, market_token) = (p.f.usdc, p.f.wsol, p.f.market_token);
    let params = p.f.decrease_params(500, 0);

    let mut plain = p.simulator.clone();
    let without_rebates = run(
        &mut plain,
        CreateOrderKind::MarketDecrease,
        &params,
        &usdc,
        None,
        Some(&wsol),
        &[market_token],
        Some(&p.position),
        ONE_PERCENT,
    )
    .unwrap();

    let mut owed = p.simulator.clone();
    p.f.add_funding_rebates(&mut owed, LONG_FUNDING_DELTA, SHORT_FUNDING_DELTA);
    let output = run(
        &mut owed,
        CreateOrderKind::MarketDecrease,
        &params,
        &usdc,
        None,
        Some(&wsol),
        &[market_token],
        Some(&p.position),
        ONE_PERCENT,
    )
    .unwrap();
    let (long, short) = rebates(&output);
    assert!(
        long > 0 && short > 0,
        "the position is owed a rebate in both tokens"
    );

    let OrderSimulationOutput::Decrease { swaps, report, .. } = &output else {
        panic!("not a decrease output");
    };
    let swapped = *swaps[0].token_out_amount();
    let profit = *report.secondary_output_amount();

    // The final output token is WSOL: the swapped collateral, the pnl output and
    // the long rebate share one receipt. The short rebate is a separate USDC one.
    let fee = output.builder_fee().unwrap();
    assert_eq!(fee.token, wsol);
    let receipts = output.receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].token, wsol);
    assert_eq!(receipts[0].gross_amount, swapped + profit + long);
    assert_eq!(receipts[0].fee_amount, fee.paid_amount);
    assert_eq!(
        receipts[0].net_amount,
        swapped + profit + long - u128::from(fee.paid_amount)
    );
    assert_eq!(receipts[1].token, usdc);
    assert_eq!(receipts[1].gross_amount, short);
    assert_eq!(receipts[1].fee_amount, 0);

    // The cap is unchanged by the rebates.
    assert_eq!(output.builder_fee(), without_rebates.builder_fee());
    assert_consistent(&output);
}

// ----------------------------------------------------------------------------
// Other kinds and failure handling
// ----------------------------------------------------------------------------

#[test]
fn swap_orders_cannot_carry_a_builder_fee() {
    let mut f = fixture();
    let (wsol, usdc, market_token) = (f.wsol, f.usdc, f.market_token);
    let params = CreateOrderParams::builder()
        .market_token(market_token)
        .is_long(true)
        .size(0)
        .amount(1_000_000)
        .build();
    let before = market_state(&f.simulator, &market_token);

    for kind in [CreateOrderKind::MarketSwap, CreateOrderKind::LimitSwap] {
        let err = error_of(run(
            &mut f.simulator,
            kind,
            &params,
            &wsol,
            Some(&usdc),
            None,
            &[market_token],
            None,
            ONE_PERCENT,
        ));
        assert!(err.contains("position orders only"), "{err}");
    }
    assert_eq!(market_state(&f.simulator, &market_token), before);

    // Without a factor the swap runs as before.
    let output = run(
        &mut f.simulator,
        CreateOrderKind::MarketSwap,
        &params,
        &wsol,
        Some(&usdc),
        None,
        &[market_token],
        None,
        0,
    )
    .unwrap();
    assert!(matches!(output, OrderSimulationOutput::Swap(_)));
    assert!(output.builder_fee().is_none());
    assert!(output.receipts().is_empty());
}

#[test]
fn a_final_output_token_without_a_price_fails_before_anything_moves() {
    let mut f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let unknown = Pubkey::new_unique();
    let params = f.decrease_params(100, 0);
    let before = market_state(&f.simulator, &market_token);

    let err = error_of(run(
        &mut f.simulator,
        CreateOrderKind::MarketDecrease,
        &params,
        &wsol,
        None,
        Some(&unknown),
        &[],
        Some(&position),
        ONE_PERCENT,
    ));
    assert!(err.contains("is not ready"), "{err}");
    assert_eq!(market_state(&f.simulator, &market_token), before);
}

#[test]
fn a_failed_fee_bearing_decrease_leaves_the_simulator_untouched() {
    // The receive token is not reachable without a swap path, which the
    // simulator only finds out after the position was decreased.
    let mut f = fixture();
    let (wsol, market_token) = (f.wsol, f.market_token);
    let position = f.open_long(&wsol, 1_000_000_000, 500);
    let params = f.decrease_params(100, 100_000_000);
    let bome: Pubkey = super::tests::BOME.parse().unwrap();
    let before = market_state(&f.simulator, &market_token);

    let attempt = |simulator: &mut Simulator, factor| {
        run(
            simulator,
            CreateOrderKind::MarketDecrease,
            &params,
            &wsol,
            None,
            Some(&bome),
            &[],
            Some(&position),
            factor,
        )
    };

    let err = error_of(attempt(&mut f.simulator, ONE_PERCENT));
    assert!(err.contains("invalid swap path"), "{err}");
    assert_eq!(market_state(&f.simulator, &market_token), before);

    // Control: without a fee the failure leaves the decrease applied. That is
    // how the simulator already behaved, and is not changed for zero factors.
    let err = error_of(attempt(&mut f.simulator, 0));
    assert!(err.contains("invalid swap path"), "{err}");
    assert_ne!(market_state(&f.simulator, &market_token), before);
}
