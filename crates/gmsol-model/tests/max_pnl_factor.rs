use gmsol_model::{
    fixed::Fixed,
    price::Prices,
    test::{MaxPnlFactors, TestMarket, TestMarketConfig, TestPosition},
    LiquidityMarketExt, LiquidityMarketMutExt, PnlFactorKind, PositionMutExt,
};
use num_traits::Zero;

#[test]
fn test_zero_max_pnl_factor_for_trader() -> gmsol_model::Result<()> {
    let unit = Fixed::<u128, 20>::ONE.into_inner();

    let price_unit = 10_u128.pow(9);

    let price_1 = 100_000 * price_unit;
    let price_2 = 150_000 * price_unit;

    let deposit_value = 1_000 * unit;
    let deposit_amount = deposit_value / price_1;

    let mut market = TestMarket::<u128, 20>::with_config(TestMarketConfig {
        max_pnl_factors: MaxPnlFactors {
            deposit: 60 * unit / 100,
            withdrawal: 60 * unit / 100,
            // Set the max pnl factor for trader to zero.
            trader: 0,
            adl: 70 * unit / 100,
        },
        ..Default::default()
    });

    let prices_1 = Prices::new_for_test(price_1, price_1, price_1);

    market.deposit(deposit_amount, 0, prices_1)?.execute()?;

    println!(
        "pool value: {}",
        market.pool_value(&prices_1, PnlFactorKind::MaxAfterDeposit, true)?
    );

    let mut position_1 = TestPosition::long(true);
    _ = position_1
        .ops(&mut market)
        .increase(prices_1, deposit_amount, deposit_value, None)?
        .execute()?;

    let prices_2 = Prices::new_for_test(price_2, price_2, price_2);
    let report = position_1
        .ops(&mut market)
        .decrease(prices_2, deposit_value, None, 0, Default::default())?
        .execute()?;

    let pnl = report.pnl();
    println!("pnl: {pnl:?}");

    // The uncapped pnl must be positive because the price is moving up.
    assert!(pnl.uncapped_pnl().is_positive());
    // Pnl must be zero due to the capping.
    assert!(pnl.pnl().is_zero());

    Ok(())
}
