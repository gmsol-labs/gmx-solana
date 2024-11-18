use std::cmp::Ordering;

use crate::{
    fixed::{Fixed, FixedPointOps},
    num::{MulDiv, Num},
};

use num_traits::{CheckedMul, One, Zero};

/// Usd value to market token amount.
///
/// Returns `None` if the computation cannot be done.
pub fn usd_to_market_token_amount<T>(
    usd_value: T,
    pool_value: T,
    supply: T,
    usd_to_amount_divisor: T,
) -> Option<T>
where
    T: MulDiv + Num,
{
    if usd_to_amount_divisor.is_zero() {
        return None;
    }
    if supply.is_zero() && pool_value.is_zero() {
        Some(usd_value / usd_to_amount_divisor)
    } else if supply.is_zero() && !pool_value.is_zero() {
        Some((pool_value.checked_add(&usd_value)?) / usd_to_amount_divisor)
    } else {
        supply.checked_mul_div(&usd_value, &pool_value)
    }
}

/// Market token amount to usd value.
///
/// Returns `None` if the computation cannot be done or `supply` is zero.
pub fn market_token_amount_to_usd<T>(amount: &T, pool_value: &T, supply: &T) -> Option<T>
where
    T: MulDiv,
{
    pool_value.checked_mul_div(amount, supply)
}

/// Apply factors using this formula: `A * x^E`.
///
/// Assuming that all values are "float"s with the same decimals.
pub fn apply_factors<T, const DECIMALS: u8>(
    value: T,
    factor: T,
    exponent_factor: T,
) -> crate::Result<T>
where
    T: FixedPointOps<DECIMALS>,
{
    Ok(apply_exponent_factor_wrapped(value, exponent_factor)
        .ok_or(crate::Error::PowComputation)?
        .checked_mul(&Fixed::from_inner(factor))
        .ok_or(crate::Error::Overflow)?
        .into_inner())
}

fn apply_exponent_factor_wrapped<T, const DECIMALS: u8>(
    value: T,
    exponent_factor: T,
) -> Option<Fixed<T, DECIMALS>>
where
    T: FixedPointOps<DECIMALS>,
{
    let unit = Fixed::ONE;
    let value = Fixed::from_inner(value);
    let exponent = Fixed::from_inner(exponent_factor);

    let ans = match value.cmp(&unit) {
        Ordering::Less => Fixed::zero(),
        Ordering::Equal => unit,
        Ordering::Greater => {
            if exponent.is_zero() {
                unit
            } else if exponent.is_one() {
                value
            } else {
                value.checked_pow(&exponent)?
            }
        }
    };
    Some(ans)
}

/// Apply exponent factor using this formula: `x^E`.
///
/// Assuming that all values are "float"s with the same decimals.
#[inline]
pub fn apply_exponent_factor<T, const DECIMALS: u8>(value: T, exponent_factor: T) -> Option<T>
where
    T: FixedPointOps<DECIMALS>,
{
    Some(apply_exponent_factor_wrapped(value, exponent_factor)?.into_inner())
}

/// Apply factor using this formula: `A * x`.
///
/// Assuming that `value` and `factor` are a fixed-point decimals,
/// but they do not need to be of the same decimals.
/// The const type `DECIMALS` is the decimals of `factor`.
#[inline]
pub fn apply_factor<T, const DECIMALS: u8>(value: &T, factor: &T) -> Option<T>
where
    T: FixedPointOps<DECIMALS>,
{
    value.checked_mul_div(factor, &FixedPointOps::UNIT)
}

/// Apply factor using this formula: `A * x`.
///
/// Assuming that `value` and `factor` are a fixed-point decimals,
/// but they do not need to be of the same decimals.
/// The const type `DECIMALS` is the decimals of `factor`.
#[inline]
pub fn apply_factor_to_signed<T, const DECIMALS: u8>(
    value: &T::Signed,
    factor: &T,
) -> Option<T::Signed>
where
    T: FixedPointOps<DECIMALS>,
{
    factor.checked_mul_div_with_signed_numerator(value, &FixedPointOps::UNIT)
}

/// Convert the `value` to a factor after dividing by the `divisor`.
///
/// ## Notes
/// - Return `zero` if the `divisor` is zero.
#[inline]
pub fn div_to_factor<T, const DECIMALS: u8>(
    value: &T,
    divisor: &T,
    round_up_magnitude: bool,
) -> Option<T>
where
    T: FixedPointOps<DECIMALS>,
{
    if divisor.is_zero() {
        return Some(T::zero());
    }

    if round_up_magnitude {
        value.checked_mul_div_ceil(&T::UNIT, divisor)
    } else {
        value.checked_mul_div(&T::UNIT, divisor)
    }
}

/// Convert the `value` to a factor after dividing by the `divisor`.
///
/// ## Notes
/// - Return `zero` if the `divisor` is zero.
#[inline]
pub fn div_to_factor_signed<T, const DECIMALS: u8>(
    value: &T::Signed,
    divisor: &T,
) -> Option<T::Signed>
where
    T: FixedPointOps<DECIMALS>,
{
    if divisor.is_zero() {
        return Some(Zero::zero());
    }

    T::UNIT.checked_mul_div_with_signed_numerator(value, divisor)
}
