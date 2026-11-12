use gmsol_model::{params::fee::FeeParams, pool::delta::BalanceChange, price::Price};

fn scub_fee_params() -> FeeParams<u64> {
    FeeParams::builder()
        .positive_impact_fee_factor(100_000_000)
        .negative_impact_fee_factor(100_000_000)
        .fee_receiver_factor(500_000_000)
        .build()
}

#[test]
fn zero_collected_fees_produce_zero_gt_credit() -> gmsol_model::Result<()> {
    let scub_price = Price { min: 100, max: 120 };
    for scub_balance_change in [BalanceChange::Improved, BalanceChange::Unchanged] {
        let scub_fees = scub_fee_params()
            .base_position_fees::<9>(&scub_price, &990, scub_balance_change)?
            .set_borrowing_fees::<9>(&500_000_000, &scub_price, 99)?;

        assert_eq!(
            *scub_fees.order_fees().fee_amounts().fee_amount_for_pool(),
            0
        );
        assert_eq!(*scub_fees.borrowing_fees().fee_amount(), 0);
        assert_eq!(*scub_fees.paid_order_and_borrowing_fee_value(), 0);
    }
    Ok(())
}

#[test]
fn gt_credit_values_rounded_collected_fees() -> gmsol_model::Result<()> {
    let scub_price = Price { min: 100, max: 120 };
    let scub_fees = scub_fee_params()
        .base_position_fees::<9>(&scub_price, &2_010, BalanceChange::Unchanged)?
        .set_borrowing_fees::<9>(&500_000_000, &scub_price, 199)?;

    assert_eq!(
        *scub_fees.order_fees().fee_amounts().fee_amount_for_pool(),
        1
    );
    assert_eq!(
        *scub_fees
            .order_fees()
            .fee_amounts()
            .fee_amount_for_receiver(),
        1
    );
    assert_eq!(*scub_fees.borrowing_fees().fee_amount(), 1);
    assert_eq!(*scub_fees.order_fees().fee_value(), 201);
    assert_eq!(*scub_fees.paid_order_and_borrowing_fee_value(), 300);
    Ok(())
}
