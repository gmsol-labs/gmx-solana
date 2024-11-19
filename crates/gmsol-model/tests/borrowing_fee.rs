use std::time::Duration;

use gmsol_model::{
    fixed::Fixed,
    params::{fee::BorrowingFeeParams, PriceImpactParams},
    price::Prices,
    test::{TestMarket, TestMarketConfig, TestPosition},
    BorrowingFeeMarket, BorrowingFeeMarketExt, LiquidityMarketMutExt, PositionMutExt,
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
            .with_positive_factor(0)
            .with_negative_factor(0)
            .with_exponent(unit)
            .build()?,
        position_impact_params: PriceImpactParams::builder()
            .with_positive_factor(0)
            .with_negative_factor(0)
            .with_exponent(unit)
            .build()?,
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
