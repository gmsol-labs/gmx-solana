use std::time::Duration;

use gmsol_model::{
    fixed::Fixed,
    num::MulDiv,
    params::{fee::BorrowingFeeParams, FeeParams, PositionParams, PriceImpactParams},
    price::Prices,
    test::{TestMarket, TestMarketConfig, TestPosition},
    Balance, BaseMarketExt, BorrowingFeeMarket, BorrowingFeeMarketExt, LiquidityMarketExt,
    LiquidityMarketMutExt, PnlFactorKind, PositionExt, PositionMutExt,
};

#[test]
fn test_total_borrowing_with_high_borrowing_factor() -> gmsol_model::Result<()> {
    let unit = Fixed::<u128, 20>::ONE.into_inner();
    let max_open_interest = 1_157_920_900_000 * unit;
    let amount = 5789604500 * 10_u128.pow(9);

    let mut market = TestMarket::<u128, 20>::with_config(TestMarketConfig {
        borrowing_fee_params: BorrowingFeeParams::builder()
            .receiver_factor(0)
            .exponent_for_long(unit)
            .exponent_for_short(unit)
            .factor_for_long(2_780_000_000_000 * 20)
            .factor_for_short(2_780_000_000_000 * 20)
            .build(),
        max_open_interest,
        swap_impact_params: PriceImpactParams::builder()
            .positive_factor(0)
            .negative_factor(0)
            .exponent(unit)
            .build(),
        position_impact_params: PriceImpactParams::builder()
            .positive_factor(0)
            .negative_factor(0)
            .exponent(unit)
            .build(),
        min_collateral_factor_for_oi: 0,
        ..Default::default()
    });

    let prices = Prices::new_for_test(20_000_000_000_000, 20_000_000_000_000, 100_000_000_000_000);

    market.deposit(amount, amount, prices)?.execute()?;

    let mut position = TestPosition::long(true);
    _ = position
        .ops(&mut market)
        .increase(prices, amount * 1_000_000, max_open_interest, None)?
        .execute()?;

    let factor = market.borrowing_factor_per_second(true, &prices)?;
    println!("borrowing factor per second: {factor}");

    market.move_clock_forward(Duration::from_secs(3600 * 24 * 365 * 100));

    _ = position
        .ops(&mut market)
        .decrease(prices, 0, None, 0, false, false)?
        .execute()?;

    let total_borrowing = market.total_borrowing_pool()?;
    println!("{total_borrowing:#?}");

    Ok(())
}

#[test]
fn test_total_borrowing_with_high_borrowing_factor_2() -> gmsol_model::Result<()> {
    const YEAR_SECONDS: u64 = 365 * 24 * 60 * 60;

    let unit = Fixed::<u128, 20>::ONE.into_inner();

    let price_decimals = 10_u128.pow(9);
    // This is just to help understand. We don't actually need to know the token decimals or unit token.
    let _unit_token = 10_u128.pow(11);

    let price = 100_000 * price_decimals;

    // Maximum depositable amount per side for each deposit operation and maximum collateral
    // amount for each position operation. Exceeding this limit will cause conversion errors
    // during signed value conversion.
    let max_deposit_amount = i128::MAX as u128 / price;

    let years = 100u64;
    let factor_per_year = 1000 / 100 * unit;

    let factor_per_second = factor_per_year / YEAR_SECONDS as u128;

    // total_borrowing == cumulative_borrowing_factor * oi / unit
    let max_oi = u128::MAX
        .checked_mul_div(&unit, &(factor_per_year * years as u128))
        .unwrap();

    // borrowing_factor_per_second == factor * (oi * unit / pool_value) / unit == factor * oi / pool_value
    let factor = factor_per_second
        .checked_mul_div(&(max_deposit_amount * price), &max_oi)
        .unwrap();

    let mut market = TestMarket::<u128, 20>::with_config(TestMarketConfig {
        max_pool_amount: u128::MAX,
        max_open_interest: u128::MAX,
        max_pool_value_for_deposit: u128::MAX,
        swap_fee_params: FeeParams::builder()
            .fee_receiver_factor(0)
            .positive_impact_fee_factor(0)
            .negative_impact_fee_factor(0)
            .build(),
        order_fee_params: FeeParams::builder()
            .fee_receiver_factor(0)
            .positive_impact_fee_factor(0)
            .negative_impact_fee_factor(0)
            .build(),
        borrowing_fee_params: BorrowingFeeParams::builder()
            .receiver_factor(0)
            .exponent_for_long(unit)
            .exponent_for_short(unit)
            .factor_for_long(factor)
            .factor_for_short(factor)
            .build(),
        swap_impact_params: PriceImpactParams::builder()
            .positive_factor(0)
            .negative_factor(0)
            .exponent(unit)
            .build(),
        position_impact_params: PriceImpactParams::builder()
            .positive_factor(0)
            .negative_factor(0)
            .exponent(unit)
            .build(),
        min_collateral_factor_for_oi: 0,
        position_params: PositionParams::builder()
            .min_collateral_value(0)
            .min_collateral_factor(0)
            .min_position_size_usd(0)
            .max_positive_position_impact_factor(5_000_000)
            .max_negative_position_impact_factor(5_000_000)
            .max_position_impact_factor_for_liquidations(2_500_000)
            .build(),
        ..Default::default()
    });

    let prices = Prices::new_for_test(price, price, price);

    // Cannot deposit assets with a total value exceeding `i128::MAX` value
    // because the pool value calculation is limited by signed values.
    market.deposit(max_deposit_amount, 0, prices)?.execute()?;

    println!(
        "pool value: {}",
        market.pool_value(&prices, PnlFactorKind::MaxAfterDeposit, true)?
    );

    // We use two positions to prevent insufficient collateral errors.
    let mut position_1 = TestPosition::long(true);
    _ = position_1
        .ops(&mut market)
        .increase(prices, max_deposit_amount, max_oi / 2, None)?
        .execute()?;

    let mut position_2 = TestPosition::long(true);
    _ = position_2
        .ops(&mut market)
        .increase(prices, max_deposit_amount, max_oi / 2, None)?
        .execute()?;

    let factor = market.borrowing_factor_per_second(true, &prices)?;
    println!("borrowing factor per second: {factor}");
    println!(
        "collateral value of position 1: {}",
        position_1.ops(&mut market).collateral_value(&prices)?
    );
    println!(
        "collateral value of position 2: {}",
        position_2.ops(&mut market).collateral_value(&prices)?
    );

    market.move_clock_forward(Duration::from_secs(YEAR_SECONDS * years));

    // Update the total borrowing pool by decreasing positions with a delta size of 0.
    _ = position_1
        .ops(&mut market)
        .decrease(prices, 0, None, 0, false, false)?
        .execute()?;

    _ = position_2
        .ops(&mut market)
        .decrease(prices, 0, None, 0, false, false)?
        .execute()?;

    let open_interest = market.open_interest()?.long_amount()?;
    let total_borrowing = market.total_borrowing_pool()?;
    let cbf = market.cumulative_borrowing_factor(true)?;
    println!("open interest: {open_interest:#?}");
    println!("total borrowing: {total_borrowing:#?}");
    println!("cumulative borrowing factor: {cbf:#?}");
    Ok(())
}
