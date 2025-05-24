/// Decimal type for price.
pub mod decimal;

pub use self::decimal::{Decimal, DecimalError};
use anchor_lang::prelude::*;

pub use ruint::aliases::U192;

/// [`U192`] number 10.
pub const TEN: U192 = U192::from_limbs([10, 0, 0]);

/// Price type.
#[derive(Debug, AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace)]
pub struct Price {
    /// Min Price.
    pub min: Decimal,
    /// Max Price.
    pub max: Decimal,
}

fn get_power_bounds() -> &'static [U192; 20] {
    const BOUNDS: [U192; 20] = [
        U192::from_limbs([18446744073709551615, 18446744073709551615, 0]),
        U192::from_limbs([18446744073709551606, 18446744073709551615, 9]),
        U192::from_limbs([18446744073709551516, 18446744073709551615, 99]),
        U192::from_limbs([18446744073709550616, 18446744073709551615, 999]),
        U192::from_limbs([18446744073709541616, 18446744073709551615, 9999]),
        U192::from_limbs([18446744073709451616, 18446744073709551615, 99999]),
        U192::from_limbs([18446744073708551616, 18446744073709551615, 999999]),
        U192::from_limbs([18446744073699551616, 18446744073709551615, 9999999]),
        U192::from_limbs([18446744073609551616, 18446744073709551615, 99999999]),
        U192::from_limbs([18446744072709551616, 18446744073709551615, 999999999]),
        U192::from_limbs([18446744063709551616, 18446744073709551615, 9999999999]),
        U192::from_limbs([18446743973709551616, 18446744073709551615, 99999999999]),
        U192::from_limbs([18446743073709551616, 18446744073709551615, 999999999999]),
        U192::from_limbs([18446734073709551616, 18446744073709551615, 9999999999999]),
        U192::from_limbs([18446644073709551616, 18446744073709551615, 99999999999999]),
        U192::from_limbs([18445744073709551616, 18446744073709551615, 999999999999999]),
        U192::from_limbs([18436744073709551616, 18446744073709551615, 9999999999999999]),
        U192::from_limbs([
            18346744073709551616,
            18446744073709551615,
            99999999999999999,
        ]),
        U192::from_limbs([
            17446744073709551616,
            18446744073709551615,
            999999999999999999,
        ]),
        U192::from_limbs([
            8446744073709551616,
            18446744073709551615,
            9999999999999999999,
        ]),
    ];

    &BOUNDS
}

/// Finds the minimum divisor decimals needed to convert a fixed-point number
/// from [`U192`] storage to [`u128`] storage.
pub fn find_divisor_decimals(num: &U192) -> u8 {
    let bounds = get_power_bounds();

    match bounds.binary_search(num) {
        Ok(idx) | Err(idx) => idx as u8,
    }
}

/// Convert to [`u128`] storage.
pub fn convert_to_u128_storage(mut num: U192, decimals: u8) -> Option<(u128, u8)> {
    let divisor_decimals = find_divisor_decimals(&num);

    if divisor_decimals > decimals {
        return None;
    }

    num /= TEN.pow(U192::from(divisor_decimals));

    Some((num.try_into().unwrap(), decimals - divisor_decimals))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds() {
        let bounds = get_power_bounds();

        assert_eq!(bounds.len(), 20);
        assert_eq!(u128::try_from(bounds[0]).unwrap(), u128::MAX);
        assert!(bounds[19].checked_mul(U192::from(10)).is_none());

        assert_eq!(find_divisor_decimals(&U192::from(u128::MAX)), 0);
        assert_eq!(find_divisor_decimals(&U192::MAX), 20);
    }

    #[test]
    fn test_convert_to_u128_storage() {
        assert_eq!(
            convert_to_u128_storage(U192::from(u128::MAX), 18),
            Some((u128::MAX, 18))
        );

        assert_eq!(
            convert_to_u128_storage(U192::from(u128::MAX) + U192::from(1), 18),
            Some((34028236692093846346337460743176821145, 17))
        );

        assert_eq!(
            convert_to_u128_storage(U192::MAX, 20),
            Some((62771017353866807638357894232076664161, 0))
        );

        assert_eq!(
            convert_to_u128_storage(
                U192::from(u128::MAX)
                    .checked_mul(U192::from(10).pow(U192::from(19)))
                    .unwrap()
                    - U192::from(11),
                20
            ),
            Some((340282366920938463463374607431768211454, 1))
        );
        assert_eq!(
            convert_to_u128_storage(
                U192::from(u128::MAX)
                    .checked_mul(U192::from(10).pow(U192::from(19)))
                    .unwrap()
                    + U192::from(1),
                20
            ),
            Some((34028236692093846346337460743176821145, 0))
        );

        assert_eq!(
            convert_to_u128_storage(
                U192::from(u128::MAX)
                    .checked_mul(U192::from(10).pow(U192::from(18)))
                    .unwrap(),
                18
            ),
            Some((340282366920938463463374607431768211455, 0))
        );
    }
}
