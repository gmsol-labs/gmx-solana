//! Builder fee effects for order simulation.
//!
//! The functions here mirror the private helpers of the store program's order
//! execution (`programs/store/src/ops/order.rs`), stage for stage:
//!
//! | Here | Program |
//! |---|---|
//! | `compute_amount` | `compute_builder_fee_amount` |
//! | `charge_on_collateral_increment` | `charge_builder_fee_on_collateral_increment` |
//! | `withdrawal_with_estimate` | `estimate_builder_fee_for_collateral_withdrawal` |
//! | `clamp_to_outputs` | `clamp_builder_fee_amount` |
//!
//! They build on the same `gmsol-model` primitives as the program
//! (`apply_factor`, `checked_round_up_div`, `Price::pick_price`), so rounding
//! agrees by construction. The program's amounts are `u64`, while the simulator
//! carries `u128`, so the narrowing the program performs is repeated explicitly
//! and fails with the same condition.
//!
//! **Keep these in step with the program.** The unit tests below replay the
//! program's own unit tests with the same inputs and expected values.
//!
//! The factor is always the one **checkpointed onto the order** by
//! `set_builder_fee`, never the one the builder currently advertises. A factor
//! of `0` means "no builder fee" and leaves every result unchanged.

use gmsol_model::{num::Unsigned, price::Price, utils::apply_factor};
use gmsol_programs::constants::MARKET_DECIMALS;
use solana_sdk::pubkey::Pubkey;

use crate::builders::order::DecreasePositionSwapType;

/// A builder fee computed for a simulated execution.
///
/// Present only when the order carries a non-zero factor. A factor that clamps
/// to a zero payment still yields an outcome, as the program's
/// `BuilderFeeCharged` event does, so its presence separates "no builder fee"
/// from "a builder fee with nothing collectible".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderFeeOutcome {
    /// The checkpointed factor that was applied.
    pub factor: u128,
    /// The token the fee is denominated in: the order's final output token,
    /// which for an increase is the position's collateral token.
    pub token: Pubkey,
    /// The computed amount, before shortfall clamping.
    pub payable_amount: u128,
    /// The amount actually charged. Always equal to `payable_amount` for an
    /// increase, because an increase fails instead of charging a partial amount.
    pub paid_amount: u64,
}

/// What the owner ends up holding in one token after a decrease.
///
/// A receipt adds up everything the execution pays out in its token: the trade
/// outputs (the final output after the receive-token swap, and the secondary pnl
/// output) and the funding rebates the position was owed. Claimable collateral,
/// which goes to separate claimable accounts, is not included.
///
/// The model's report keeps the **gross** payout, because the program does not
/// net the fee out of the transferred output: the whole output lands in the
/// order's escrow and `settle_builder_fee` later moves the fee to the builder.
/// The receipt is where the fee is taken out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenReceipt {
    /// The token.
    pub token: Pubkey,
    /// The amount paid out in this token by the execution, before the builder
    /// fee.
    pub gross_amount: u128,
    /// The builder fee reserved from this token.
    pub fee_amount: u64,
    /// `gross_amount - fee_amount`.
    pub net_amount: u128,
}

fn overflow() -> crate::Error {
    crate::Error::custom("[sim] TokenAmountOverflow: builder fee computation overflowed")
}

fn narrow(amount: u128) -> crate::Result<u64> {
    u64::try_from(amount).map_err(|_| overflow())
}

/// Computes a builder fee amount, in the token of `price`, from the size delta
/// and the checkpointed `factor`.
///
/// Rounds up so the fee is never under-collected, using the minimum price.
/// Returns `0` for a zero `factor` without touching `price`.
pub(crate) fn compute_amount(
    size_delta_usd: u128,
    factor: u128,
    price: &Price<u128>,
) -> crate::Result<u128> {
    if factor == 0 {
        return Ok(0);
    }
    let fee_value =
        apply_factor::<_, { MARKET_DECIMALS }>(&size_delta_usd, &factor).ok_or_else(overflow)?;
    fee_value
        .checked_round_up_div(price.pick_price(false))
        .ok_or_else(overflow)
}

/// Charges the fee against an increase order's swapped collateral.
///
/// Returns the collateral left for the position and the fee. A zero `factor`
/// returns `gross` unchanged and no fee.
///
/// # Errors
/// Fails when `gross` cannot cover the fee (`BuilderFeeExceedsCollateral` on
/// chain). A fee that consumes exactly `gross` is allowed and leaves `0`.
pub(crate) fn charge_on_collateral_increment(
    gross: u128,
    size_delta_usd: u128,
    factor: u128,
    collateral_token: &Pubkey,
    collateral_price: &Price<u128>,
) -> crate::Result<(u128, Option<BuilderFeeOutcome>)> {
    if factor == 0 {
        return Ok((gross, None));
    }
    let payable_amount = compute_amount(size_delta_usd, factor, collateral_price)?;
    let paid_amount = narrow(payable_amount)?;
    if gross < u128::from(paid_amount) {
        return Err(crate::Error::custom(format!(
            "[sim] BuilderFeeExceedsCollateral: the builder fee ({paid_amount}) exceeds the collateral \
             available after the swap ({gross}), so the order would fail on execution"
        )));
    }
    Ok((
        gross - u128::from(paid_amount),
        Some(BuilderFeeOutcome {
            factor,
            token: *collateral_token,
            payable_amount,
            paid_amount,
        }),
    ))
}

/// Tops up a decrease order's collateral withdrawal by an estimate of the fee,
/// so that it is still funded when the position closes at a loss.
///
/// The estimate uses the **requested** size and the **collateral** price. It
/// only sizes the withdrawal. The fee actually charged is computed afterwards
/// from the executed size in the final output token.
///
/// Rejects `CollateralToPnlToken` for any non-zero `factor`, whatever the
/// estimate rounds to: the executed size may be enlarged into a full close and
/// incur a fee the swap would then have bypassed.
pub(crate) fn withdrawal_with_estimate(
    collateral_withdrawal_amount: u128,
    size_delta_usd: u128,
    factor: u128,
    collateral_price: &Price<u128>,
    swap_type: Option<DecreasePositionSwapType>,
) -> crate::Result<u128> {
    if factor == 0 {
        return Ok(collateral_withdrawal_amount);
    }
    if matches!(
        swap_type,
        Some(DecreasePositionSwapType::CollateralToPnlToken)
    ) {
        return Err(crate::Error::custom(
            "[sim] BuilderFeeSwapTypeNotAllowed: `CollateralToPnlToken` cannot be used with a \
             non-zero builder fee factor",
        ));
    }
    let estimate = compute_amount(size_delta_usd, factor, collateral_price)?;
    collateral_withdrawal_amount
        .checked_add(estimate)
        .ok_or_else(overflow)
}

/// Clamps a computed decrease fee to the outputs that fund the final output
/// token's escrow.
///
/// A secondary output counts only when it is in the final output token. Both
/// amounts are narrowed to `u64` whichever token they are in, as the program
/// does when it reads the report, and the combined amount is overflow-checked.
pub(crate) fn clamp_to_outputs(
    payable_amount: u128,
    final_output_token: &Pubkey,
    output_amount: u128,
    secondary_output_token: &Pubkey,
    secondary_output_amount: u128,
) -> crate::Result<u64> {
    let output_amount = narrow(output_amount)?;
    let secondary_output_amount = narrow(secondary_output_amount)?;
    let available = if final_output_token == secondary_output_token {
        output_amount
            .checked_add(secondary_output_amount)
            .ok_or_else(overflow)?
    } else {
        output_amount
    };
    // `available` fits in `u64`, so the minimum does too.
    narrow(payable_amount.min(u128::from(available)))
}

/// Builds the per-token receipts of a decrease.
///
/// `other_outputs` are the remaining amounts the execution pays out, as
/// `(token, amount)` pairs: the secondary (pnl token) output and the funding
/// rebates in the market's long and short tokens. They are merged by token, in
/// the order given, with the final output token's own amount.
///
/// The final output token always has the first entry, whatever its amount.
/// Amounts in the same token share one entry, because they land in the same
/// escrow and the fee is taken from their sum. Any other token gets its own
/// entry (none when its amount is zero), and never carries the fee.
///
/// This only reports what is paid out. The cap on the fee itself is computed
/// separately from the trade outputs, as the program does, and is not widened
/// by the funding rebates listed here.
pub(crate) fn build_receipts(
    final_output_token: &Pubkey,
    output_amount: u128,
    other_outputs: &[(Pubkey, u128)],
    paid_amount: u64,
) -> crate::Result<Vec<TokenReceipt>> {
    let mut amounts: Vec<(Pubkey, u128)> = Vec::with_capacity(1 + other_outputs.len());
    amounts.push((*final_output_token, output_amount));
    for (token, amount) in other_outputs {
        if *amount == 0 {
            continue;
        }
        match amounts.iter().position(|entry| entry.0 == *token) {
            Some(idx) => {
                amounts[idx].1 = amounts[idx].1.checked_add(*amount).ok_or_else(overflow)?;
            }
            None => amounts.push((*token, *amount)),
        }
    }
    amounts
        .into_iter()
        .enumerate()
        // The fee is reserved from the final output token, which is first.
        .map(|(idx, (token, gross_amount))| {
            receipt(token, gross_amount, if idx == 0 { paid_amount } else { 0 })
        })
        .collect()
}

fn receipt(token: Pubkey, gross_amount: u128, fee_amount: u64) -> crate::Result<TokenReceipt> {
    let net_amount = gross_amount
        .checked_sub(u128::from(fee_amount))
        .ok_or_else(|| crate::Error::custom("[sim] the builder fee exceeds the gross output"))?;
    Ok(TokenReceipt {
        token,
        gross_amount,
        fee_amount,
        net_amount,
    })
}

#[cfg(test)]
mod tests {
    use gmsol_programs::constants::MARKET_USD_UNIT;

    use super::*;

    fn price(min: u128, max: u128) -> Price<u128> {
        Price { min, max }
    }

    fn charge(
        gross: u128,
        size_delta_usd: u128,
        factor: u128,
        unit_price: u128,
    ) -> crate::Result<(u128, Option<BuilderFeeOutcome>)> {
        charge_on_collateral_increment(
            gross,
            size_delta_usd,
            factor,
            &Pubkey::new_unique(),
            &price(unit_price, unit_price),
        )
    }

    // The five parameters of the program's tests: $100,000 size, 5 bps factor
    // => $50 fee value.
    const SIZE: u128 = 100_000 * MARKET_USD_UNIT;
    const FACTOR: u128 = MARKET_USD_UNIT / 2_000;
    const TWO: u128 = 2 * MARKET_USD_UNIT;

    #[test]
    fn zero_factor_yields_zero_fee_without_needing_a_price() {
        assert_eq!(compute_amount(1_000_000, 0, &price(0, 0)).unwrap(), 0);
    }

    #[test]
    fn computes_expected_amount() {
        // $50 at $2/unit => 25 units.
        assert_eq!(compute_amount(SIZE, FACTOR, &price(TWO, TWO)).unwrap(), 25);
    }

    #[test]
    fn amount_rounds_up_on_remainder() {
        // 50 / 3 = 16.67, rounded up.
        let three = 3 * MARKET_USD_UNIT;
        assert_eq!(
            compute_amount(SIZE, FACTOR, &price(three, three)).unwrap(),
            17
        );
    }

    #[test]
    fn amount_uses_the_minimum_price() {
        // $50 at a $2 minimum (not the $4 maximum) => 25 units.
        assert_eq!(
            compute_amount(SIZE, FACTOR, &price(TWO, 2 * TWO)).unwrap(),
            25
        );
    }

    #[test]
    fn a_zero_price_is_an_error_not_a_zero_fee() {
        assert!(compute_amount(SIZE, FACTOR, &price(0, 0)).is_err());
    }

    #[test]
    fn an_overflowing_fee_value_is_an_error() {
        assert!(compute_amount(u128::MAX, u128::MAX, &price(TWO, TWO)).is_err());
    }

    #[test]
    fn clamps_to_available() {
        let token = Pubkey::new_unique();
        assert_eq!(
            clamp_to_outputs(1_000, &token, 100, &token, 0).unwrap(),
            100
        );
    }

    #[test]
    fn clamps_to_zero_when_nothing_is_available() {
        let token = Pubkey::new_unique();
        assert_eq!(clamp_to_outputs(1_000, &token, 0, &token, 0).unwrap(), 0);
    }

    #[test]
    fn within_available_is_unchanged() {
        let token = Pubkey::new_unique();
        assert_eq!(clamp_to_outputs(10, &token, 100, &token, 0).unwrap(), 10);
    }

    #[test]
    fn counts_same_token_secondary_output() {
        let token = Pubkey::new_unique();
        // The primary output alone cannot cover the fee, but both outputs can.
        assert_eq!(clamp_to_outputs(50, &token, 20, &token, 80).unwrap(), 50);
    }

    #[test]
    fn shortfall_clamps_to_combined_outputs() {
        let token = Pubkey::new_unique();
        assert_eq!(clamp_to_outputs(150, &token, 20, &token, 80).unwrap(), 100);
    }

    #[test]
    fn can_be_funded_only_by_secondary_output() {
        let token = Pubkey::new_unique();
        assert_eq!(clamp_to_outputs(50, &token, 0, &token, 80).unwrap(), 50);
    }

    #[test]
    fn ignores_different_token_secondary_output() {
        let final_token = Pubkey::new_unique();
        let secondary_token = Pubkey::new_unique();
        // Even a maximum secondary balance cannot fund a fee in another token,
        // nor trigger overflow.
        for primary_amount in [0u64, 20] {
            assert_eq!(
                clamp_to_outputs(
                    50,
                    &final_token,
                    primary_amount.into(),
                    &secondary_token,
                    u64::MAX.into(),
                )
                .unwrap(),
                primary_amount.min(50)
            );
        }
    }

    #[test]
    fn accepts_maximum_combined_output() {
        let token = Pubkey::new_unique();
        assert_eq!(
            clamp_to_outputs(u128::MAX, &token, (u64::MAX - 1).into(), &token, 1,).unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn rejects_combined_output_overflow() {
        let token = Pubkey::new_unique();
        assert!(clamp_to_outputs(50, &token, u64::MAX.into(), &token, 1).is_err());
    }

    #[test]
    fn rejects_outputs_wider_than_u64() {
        let token = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let too_big = u128::from(u64::MAX) + 1;
        assert!(clamp_to_outputs(50, &token, too_big, &other, 0).is_err());
        // The secondary is narrowed even when it cannot fund the fee.
        assert!(clamp_to_outputs(50, &token, 0, &other, too_big).is_err());
    }

    #[test]
    fn zero_factor_leaves_collateral_increment_unchanged() {
        let (net, fee) = charge(1_000, SIZE, 0, TWO).unwrap();
        assert_eq!(net, 1_000);
        assert_eq!(fee, None);
    }

    #[test]
    fn charges_expected_fee_on_collateral_increment() {
        let (net, fee) = charge(1_000, SIZE, FACTOR, TWO).unwrap();
        let fee = fee.unwrap();
        assert_eq!(fee.paid_amount, 25);
        assert_eq!(fee.payable_amount, 25);
        assert_eq!(fee.factor, FACTOR);
        assert_eq!(net, 975);
    }

    #[test]
    fn exact_fee_consumes_the_whole_collateral_increment() {
        let (net, fee) = charge(25, SIZE, FACTOR, TWO).unwrap();
        assert_eq!(fee.unwrap().paid_amount, 25);
        assert_eq!(net, 0);
    }

    #[test]
    fn underpayment_errors_instead_of_clamping() {
        let err = charge(10, SIZE, FACTOR, TWO).unwrap_err();
        assert!(
            err.to_string().contains("BuilderFeeExceedsCollateral"),
            "{err}"
        );
    }

    #[test]
    fn a_fee_wider_than_u64_is_an_overflow_not_a_shortfall() {
        // A 100% factor on a huge size at the smallest price: the fee exceeds `u64`.
        let err = charge(u128::MAX, 10u128.pow(30), MARKET_USD_UNIT, 1).unwrap_err();
        assert!(err.to_string().contains("TokenAmountOverflow"), "{err}");
    }

    #[test]
    fn zero_factor_leaves_withdrawal_amount_unchanged() {
        let amount = withdrawal_with_estimate(1_000, SIZE, 0, &price(TWO, TWO), None).unwrap();
        assert_eq!(amount, 1_000);
    }

    #[test]
    fn tops_up_withdrawal_amount_by_the_estimate() {
        let amount = withdrawal_with_estimate(
            1_000,
            SIZE,
            FACTOR,
            &price(TWO, TWO),
            Some(DecreasePositionSwapType::PnlTokenToCollateralToken),
        )
        .unwrap();
        // The withdrawal grows by the estimate; it is not subtracted.
        assert_eq!(amount, 1_025);
    }

    #[test]
    fn no_swap_and_unset_swap_type_are_allowed_with_a_fee() {
        for swap_type in [None, Some(DecreasePositionSwapType::NoSwap)] {
            assert!(
                withdrawal_with_estimate(1_000, SIZE, FACTOR, &price(TWO, TWO), swap_type).is_ok()
            );
        }
    }

    #[test]
    fn collateral_to_pnl_token_is_rejected_with_a_fee() {
        let err = withdrawal_with_estimate(
            1_000,
            SIZE,
            FACTOR,
            &price(TWO, TWO),
            Some(DecreasePositionSwapType::CollateralToPnlToken),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("BuilderFeeSwapTypeNotAllowed"),
            "{err}"
        );
    }

    #[test]
    fn collateral_to_pnl_token_is_rejected_when_the_estimate_rounds_to_zero() {
        // A dust size delta whose fee value truncates to zero while the factor
        // is set: the executed size may still be enlarged into a full close.
        let err = withdrawal_with_estimate(
            1_000,
            1_000,
            FACTOR,
            &price(TWO, TWO),
            Some(DecreasePositionSwapType::CollateralToPnlToken),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("BuilderFeeSwapTypeNotAllowed"),
            "{err}"
        );
    }

    #[test]
    fn collateral_to_pnl_token_is_allowed_when_the_factor_is_zero() {
        assert!(withdrawal_with_estimate(
            1_000,
            SIZE,
            0,
            &price(TWO, TWO),
            Some(DecreasePositionSwapType::CollateralToPnlToken),
        )
        .is_ok());
    }

    #[test]
    fn withdrawal_top_up_overflow_is_an_error() {
        assert!(withdrawal_with_estimate(u128::MAX, SIZE, FACTOR, &price(TWO, TWO), None).is_err());
    }

    #[test]
    fn same_token_receipts_merge_and_carry_the_fee() {
        let token = Pubkey::new_unique();
        let receipts = build_receipts(&token, 20, &[(token, 80)], 50).unwrap();
        assert_eq!(
            receipts,
            vec![TokenReceipt {
                token,
                gross_amount: 100,
                fee_amount: 50,
                net_amount: 50,
            }]
        );
    }

    #[test]
    fn different_token_receipts_keep_the_fee_on_the_final_token() {
        let final_token = Pubkey::new_unique();
        let secondary_token = Pubkey::new_unique();
        let receipts = build_receipts(&final_token, 20, &[(secondary_token, 80)], 20).unwrap();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].token, final_token);
        assert_eq!(
            (
                receipts[0].gross_amount,
                receipts[0].fee_amount,
                receipts[0].net_amount
            ),
            (20, 20, 0)
        );
        assert_eq!(receipts[1].token, secondary_token);
        assert_eq!(
            (
                receipts[1].gross_amount,
                receipts[1].fee_amount,
                receipts[1].net_amount
            ),
            (80, 0, 80)
        );
    }

    #[test]
    fn an_empty_secondary_output_in_another_token_is_omitted() {
        let final_token = Pubkey::new_unique();
        let receipts = build_receipts(&final_token, 20, &[(Pubkey::new_unique(), 0)], 0).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].net_amount, 20);
    }

    #[test]
    fn the_final_token_always_has_a_receipt() {
        let token = Pubkey::new_unique();
        let receipts = build_receipts(&token, 0, &[(Pubkey::new_unique(), 0)], 0).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].gross_amount, 0);
    }

    #[test]
    fn a_fee_larger_than_the_gross_output_is_rejected() {
        let token = Pubkey::new_unique();
        assert!(build_receipts(&token, 10, &[(Pubkey::new_unique(), 0)], 11).is_err());
    }

    #[test]
    fn funding_rebates_in_the_final_token_join_its_receipt() {
        let final_token = Pubkey::new_unique();
        let pnl_token = Pubkey::new_unique();
        let other_token = Pubkey::new_unique();
        // The final token also receives a long funding rebate; the pnl token has
        // its own output; the short funding rebate is in a third token.
        let receipts = build_receipts(
            &final_token,
            100,
            &[(pnl_token, 30), (final_token, 7), (other_token, 5)],
            40,
        )
        .unwrap();
        assert_eq!(
            receipts,
            vec![
                TokenReceipt {
                    token: final_token,
                    gross_amount: 107,
                    fee_amount: 40,
                    net_amount: 67,
                },
                TokenReceipt {
                    token: pnl_token,
                    gross_amount: 30,
                    fee_amount: 0,
                    net_amount: 30,
                },
                TokenReceipt {
                    token: other_token,
                    gross_amount: 5,
                    fee_amount: 0,
                    net_amount: 5,
                },
            ]
        );
    }

    #[test]
    fn funding_rebates_merge_with_a_secondary_output_in_the_same_token() {
        let final_token = Pubkey::new_unique();
        let pnl_token = Pubkey::new_unique();
        let other_token = Pubkey::new_unique();
        // The pnl output and the long funding rebate are both in `pnl_token`.
        let receipts = build_receipts(
            &final_token,
            10,
            &[(pnl_token, 30), (pnl_token, 7), (other_token, 5)],
            0,
        )
        .unwrap();
        assert_eq!(receipts.len(), 3);
        assert_eq!(receipts[0].token, final_token);
        assert_eq!(receipts[0].gross_amount, 10);
        assert_eq!(receipts[1].token, pnl_token);
        assert_eq!(receipts[1].gross_amount, 37);
        assert_eq!(receipts[2].token, other_token);
        assert_eq!(receipts[2].gross_amount, 5);
    }

    #[test]
    fn a_funding_rebate_can_be_the_only_payout_in_its_token() {
        // The position pays out only collateral in `final_token`, yet is owed a
        // rebate in another token: that token must not be missing.
        let final_token = Pubkey::new_unique();
        let rebate_token = Pubkey::new_unique();
        let receipts = build_receipts(&final_token, 10, &[(rebate_token, 9)], 0).unwrap();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[1].token, rebate_token);
        assert_eq!(receipts[1].gross_amount, 9);
        assert_eq!(receipts[1].net_amount, 9);
    }

    #[test]
    fn long_and_short_rebates_of_a_single_token_market_are_one_receipt() {
        // In a market whose long and short tokens are the same mint, both
        // rebates (and the output) land in the same escrow.
        let token = Pubkey::new_unique();
        let receipts = build_receipts(&token, 10, &[(token, 3), (token, 4)], 5).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].gross_amount, 17);
        assert_eq!(receipts[0].fee_amount, 5);
        assert_eq!(receipts[0].net_amount, 12);
    }

    #[test]
    fn zero_funding_rebates_add_no_receipt() {
        let final_token = Pubkey::new_unique();
        let receipts = build_receipts(
            &final_token,
            10,
            &[
                (Pubkey::new_unique(), 0),
                (final_token, 0),
                (Pubkey::new_unique(), 0),
            ],
            0,
        )
        .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].gross_amount, 10);
    }

    #[test]
    fn only_the_final_token_carries_the_fee_even_with_rebates_elsewhere() {
        let final_token = Pubkey::new_unique();
        let rebate_token = Pubkey::new_unique();
        let receipts = build_receipts(&final_token, 50, &[(rebate_token, 500)], 20).unwrap();
        assert_eq!(receipts[0].fee_amount, 20);
        assert_eq!(receipts[1].fee_amount, 0);
        // A large rebate in another token neither pays the fee nor changes it.
        assert_eq!(receipts[0].net_amount, 30);
        assert_eq!(receipts[1].net_amount, 500);
    }

    #[test]
    fn rebates_can_not_hide_a_fee_larger_than_the_final_token_total() {
        // The fee is reserved from the final output token's receipt only.
        let final_token = Pubkey::new_unique();
        let rebate_token = Pubkey::new_unique();
        assert!(build_receipts(&final_token, 10, &[(rebate_token, 500)], 11).is_err());
    }

    #[test]
    fn merging_receipts_overflow_is_an_error() {
        let token = Pubkey::new_unique();
        assert!(build_receipts(&token, u128::MAX, &[(token, 1)], 0).is_err());
    }
}
