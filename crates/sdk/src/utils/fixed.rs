use crate::constants::MARKET_DECIMALS;
use rust_decimal::Decimal;

const MAX_REPR: u128 = 0x0000_0000_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF;
const TARGET_SCALE: u32 = MAX_REPR.ilog10() - 1;

/// Convert signed USD value to [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::signed_value_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     signed_value_to_decimal(-429_663_361_044_608_151),
///     dec!(-0.00429663361044608151),
/// );
/// ```
pub fn signed_value_to_decimal(num: i128) -> Decimal {
    signed_fixed_to_decimal(num, MARKET_DECIMALS).expect("must be `Some`")
}

/// Convert unsigned USD value to [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::unsigned_value_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     unsigned_value_to_decimal(429_663_361_044_608_151),
///     dec!(0.00429663361044608151),
/// );
/// ```
pub fn unsigned_value_to_decimal(num: u128) -> Decimal {
    unsigned_fixed_to_decimal(num, MARKET_DECIMALS).expect("must be `Some`")
}

/// Convert unsigned fixed-point amount to [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::unsigned_amount_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     unsigned_amount_to_decimal(100_451_723_195, 6),
///     dec!(100_451.723195),
/// );
/// ```
pub fn unsigned_amount_to_decimal(mut num: u64, mut decimals: u8) -> Decimal {
    const MAX_DECIMALS: u8 = 28;
    const MAX_SCALE_FOR_U64: u8 = 19;

    if decimals > MAX_DECIMALS {
        let scale_diff = decimals - MAX_DECIMALS;
        if scale_diff > MAX_SCALE_FOR_U64 {
            return Decimal::ZERO;
        }
        num /= 10u64.pow(scale_diff as u32);
        decimals = MAX_DECIMALS;
    }

    unsigned_fixed_to_decimal(num as u128, decimals).expect("must be `Some`")
}

/// Convert signed fixed-point amount to [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::signed_amount_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     signed_amount_to_decimal(-100_451_723_195, 6),
///     dec!(-100_451.723195),
/// );
/// ```
pub fn signed_amount_to_decimal(num: i64, decimals: u8) -> Decimal {
    let is_negative = num.is_negative();
    let d = unsigned_amount_to_decimal(num.unsigned_abs(), decimals);
    if is_negative {
        -d
    } else {
        d
    }
}

/// Convert unsigned fixed-point number to [`Decimal`].
///
/// Returns `None` if it cannot be represented as a [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::unsigned_fixed_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     unsigned_fixed_to_decimal(100_451_723_195, 6),
///     Some(dec!(100_451.723195)),
/// );
///
/// assert_eq!(
///     unsigned_fixed_to_decimal(u128::MAX, 10),
///     None,
/// );
/// ```
pub fn unsigned_fixed_to_decimal(num: u128, decimals: u8) -> Option<Decimal> {
    fn convert_by_change_the_scale(mut num: u128, scale: u32) -> Option<Decimal> {
        let digits = num.ilog10();
        debug_assert!(digits >= TARGET_SCALE);
        let scale_diff = digits - TARGET_SCALE;
        if scale < scale_diff {
            return None;
        }
        num /= 10u128.pow(scale_diff);
        Some(Decimal::from_i128_with_scale(
            num as i128,
            scale - scale_diff,
        ))
    }

    let scale = decimals as u32;
    if num > MAX_REPR {
        convert_by_change_the_scale(num, scale)
    } else {
        Decimal::try_from_i128_with_scale(num as i128, scale).ok()
    }
}

/// Convert signed fixed-point value to [`Decimal`].
///
/// Returns `None` if it cannot be represented as a [`Decimal`].
///
/// # Examples
///
/// ```
/// use gmsol::utils::signed_fixed_to_decimal;
/// use rust_decimal_macros::dec;
///
/// assert_eq!(
///     signed_fixed_to_decimal(-100_451_723_195, 6),
///     Some(dec!(-100_451.723195)),
/// );
///
/// assert_eq!(
///     signed_fixed_to_decimal(i128::MIN, 10),
///     None,
/// );
/// ```
pub fn signed_fixed_to_decimal(num: i128, decimals: u8) -> Option<Decimal> {
    let is_negative = num.is_negative();
    let d = unsigned_fixed_to_decimal(num.unsigned_abs(), decimals)?;
    if is_negative {
        Some(-d)
    } else {
        Some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_convert_value_to_decimal() {
        assert_eq!(
            signed_value_to_decimal(2_268_944_690_310_400_000_000_000),
            dec!(22689.446903104)
        );
        assert_eq!(
            signed_value_to_decimal(i128::MAX),
            dec!(1701411834604692317.316873037),
        );
        assert_eq!(
            signed_value_to_decimal(i128::MIN),
            dec!(-1701411834604692317.316873037)
        );
        assert_eq!(
            signed_value_to_decimal(0x0000_0000_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            dec!(792281625.14264337593543950335),
        );

        assert_eq!(
            unsigned_value_to_decimal(u128::MAX),
            dec!(3402823669209384634.633746074),
        );
        assert_eq!(
            unsigned_value_to_decimal(0x0000_0000_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            dec!(792281625.14264337593543950335),
        );
    }

    #[test]
    fn test_convert_signed_amount_to_decimal() {
        assert_eq!(
            signed_amount_to_decimal(-2_649_941_038_310_943, 6),
            dec!(-2649941038.310943),
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MAX, 0),
            dec!(9223372036854775807),
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MIN, 0),
            dec!(-9223372036854775808),
        );

        assert_eq!(
            signed_amount_to_decimal(1, 28),
            dec!(0.0000000000000000000000000001),
        );

        assert_eq!(
            signed_amount_to_decimal(-1, 28),
            dec!(-0.0000000000000000000000000001),
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MAX, 29),
            dec!(0.000000000092233720368547758)
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MIN, 29),
            dec!(-0.000000000092233720368547758),
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MAX, 46),
            dec!(0.0000000000000000000000000009),
        );

        assert_eq!(
            signed_amount_to_decimal(i64::MIN, 46),
            dec!(-0.0000000000000000000000000009),
        );

        assert_eq!(signed_amount_to_decimal(i64::MAX, 47), Decimal::ZERO);

        assert_eq!(signed_amount_to_decimal(i64::MIN, 47), Decimal::ZERO);
    }

    #[test]
    fn test_convert_unsigned_amount_to_decimal() {
        assert_eq!(
            unsigned_amount_to_decimal(2_649_941_038_310_943, 6),
            dec!(2649941038.310943),
        );

        assert_eq!(
            unsigned_amount_to_decimal(u64::MAX, 0),
            dec!(18446744073709551615),
        );

        assert_eq!(
            unsigned_amount_to_decimal(u64::MAX, 29),
            dec!(0.0000000001844674407370955161),
        );

        assert_eq!(
            unsigned_amount_to_decimal(u64::MAX, 47),
            dec!(0.0000000000000000000000000001)
        );

        assert_eq!(unsigned_amount_to_decimal(u64::MAX, 48), Decimal::ZERO);
    }
}
