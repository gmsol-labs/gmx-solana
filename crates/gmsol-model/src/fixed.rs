use std::ops::{Add, Mul};

use num_traits::{CheckedAdd, CheckedMul, One, Zero};

use crate::num::{MulDiv, Num};

/// Number type with the required properties for implementing [`Fixed`].
pub trait FixedPointOps<const DECIMALS: u8>: MulDiv + Num {
    /// The unit value (i.e. the value "one") which is expected to be `pow(10, DECIMALS)`.
    const UNIT: Self;

    /// Fixed point power.
    fn checked_pow_fixed(&self, exponent: &Self) -> Option<Self>;
}

impl<const DECIMALS: u8> FixedPointOps<DECIMALS> for u64 {
    const UNIT: Self = 10u64.pow(DECIMALS as u32);

    fn checked_pow_fixed(&self, exponent: &Self) -> Option<Self> {
        use rust_decimal::{Decimal, MathematicalOps};

        let unit = <Self as FixedPointOps<DECIMALS>>::UNIT;
        if *exponent % unit == 0 {
            let exp = exponent / unit;
            // FIXME: use a better algorithm.
            let mut ans = Fixed::<Self, DECIMALS>::one();
            let base = Fixed::<Self, DECIMALS>::from_inner(*self);
            for _ in 0..exp {
                ans = ans.checked_mul(&base)?;
            }
            return Some(ans.0);
        }

        // `scale > 28` is not supported by `rust_decimal`.
        if DECIMALS > 28 {
            return None;
        }
        let value = Decimal::new((*self).try_into().ok()?, DECIMALS as u32);
        let exponent = Decimal::new((*exponent).try_into().ok()?, DECIMALS as u32);
        let mut ans = value.checked_powd(exponent)?;
        ans.rescale(DECIMALS as u32);
        ans.mantissa().try_into().ok()
    }
}

#[cfg(feature = "u128")]
impl<const DECIMALS: u8> FixedPointOps<DECIMALS> for u128 {
    const UNIT: Self = 10u128.pow(DECIMALS as u32);

    fn checked_pow_fixed(&self, exponent: &Self) -> Option<Self> {
        use std::cmp::Ordering;

        let unit = <Self as FixedPointOps<DECIMALS>>::UNIT;
        if *exponent % unit == 0 {
            let exp = exponent / unit;
            // FIXME: use a better algorithm.
            let mut ans = Fixed::<Self, DECIMALS>::one();
            let base = Fixed::<Self, DECIMALS>::from_inner(*self);
            for _ in 0..exp {
                ans = ans.checked_mul(&base)?;
            }
            return Some(ans.0);
        }

        type Convert = U64D9;

        let (divisor, multiplier) = match DECIMALS.cmp(&U64D9::DECIMALS) {
            Ordering::Greater => {
                let divisor = 10u128.pow((DECIMALS - Convert::DECIMALS) as u32);
                (Some(divisor), None)
            }
            Ordering::Less => {
                let multiplier = 10u128.pow((Convert::DECIMALS - DECIMALS) as u32);
                (None, Some(multiplier))
            }
            Ordering::Equal => (None, None),
        };
        let convert_to = |value: Self| -> Option<u64> {
            match (&divisor, &multiplier) {
                (Some(divisor), _) => (value / *divisor).try_into().ok(),
                (_, Some(multiplier)) => value.checked_mul(*multiplier)?.try_into().ok(),
                _ => value.try_into().ok(),
            }
        };
        let convert_from = |value: u64| -> Option<Self> {
            let value: Self = value.into();
            match (&divisor, &multiplier) {
                (Some(divisor), _) => value.checked_mul(*divisor),
                (_, Some(multiplier)) => Some(value / *multiplier),
                _ => Some(value),
            }
        };
        let ans = FixedPointOps::<{ Convert::DECIMALS }>::checked_pow_fixed(
            &convert_to(*self)?,
            &convert_to(*exponent)?,
        )?;
        convert_from(ans)
    }
}

/// Fixed-point decimal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Fixed<T, const DECIMALS: u8>(T);

impl<T, const DECIMALS: u8> Fixed<T, DECIMALS> {
    /// Get the internal integer representation.
    pub fn get(&self) -> &T {
        &self.0
    }

    /// Create a new decimal from the inner representation.
    #[inline]
    pub fn from_inner(inner: T) -> Self {
        Self(inner)
    }

    /// Get the inner value.
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> Fixed<T, DECIMALS> {
    /// The unit value.
    pub const ONE: Fixed<T, DECIMALS> = Fixed(FixedPointOps::UNIT);
    /// The decimals.
    pub const DECIMALS: u8 = DECIMALS;

    /// Checked pow.
    pub fn checked_pow(&self, exponent: &Self) -> Option<Self> {
        let inner = self.0.checked_pow_fixed(&exponent.0)?;
        Some(Self(inner))
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> Add for Fixed<T, DECIMALS> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> CheckedAdd for Fixed<T, DECIMALS> {
    fn checked_add(&self, v: &Self) -> Option<Self> {
        Some(Self(self.0.checked_add(&v.0)?))
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> Mul for Fixed<T, DECIMALS> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.checked_mul(&rhs).expect("invalid multiplication")
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> CheckedMul for Fixed<T, DECIMALS> {
    fn checked_mul(&self, v: &Self) -> Option<Self> {
        Some(Self(self.0.checked_mul_div(&v.0, &Self::ONE.0)?))
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> Zero for Fixed<T, DECIMALS> {
    fn zero() -> Self {
        Self(T::zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl<T: FixedPointOps<DECIMALS>, const DECIMALS: u8> One for Fixed<T, DECIMALS> {
    fn one() -> Self {
        Self::ONE
    }

    fn is_one(&self) -> bool
    where
        Self: PartialEq,
    {
        self.0 == Self::ONE.0
    }
}

/// Decimal type with `9` decimals and backed by [`u64`]
pub type U64D9 = Fixed<u64, 9>;

#[cfg(feature = "u128")]
/// Decimal type with `20` decimals and backed by [`u128`]
pub type U128D20 = Fixed<u128, 20>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let x = U64D9::from_inner(12_800_000_000);
        let y = U64D9::from_inner(25_600_000_001);
        assert_eq!(x * y, U64D9::from_inner(327_680_000_012));
    }

    #[test]
    fn pow() {
        let x = U64D9::from_inner(123_456 * 100_000_000);
        let exp = U64D9::from_inner(11 * 100_000_000);
        let ans = x.checked_pow(&exp).unwrap();
        assert_eq!(ans, U64D9::from_inner(31670982733137));
    }

    #[cfg(feature = "u128")]
    #[test]
    fn basic_u128() {
        let x = U128D20::from_inner(128 * U128D20::ONE.0);
        let y = U128D20::from_inner(256 * U128D20::ONE.0 + 1);
        assert_eq!(
            x * y,
            U128D20::from_inner(3_276_800_000_000_000_000_000_128)
        );
    }

    #[cfg(feature = "u128")]
    #[test]
    fn pow_u128() {
        let x = U128D20::from_inner(123_456 * U128D20::ONE.0 / 10);
        let exp = U128D20::from_inner(11 * U128D20::ONE.0 / 10);
        let ans = x.checked_pow(&exp).unwrap();
        assert_eq!(ans, U128D20::from_inner(3167098273313700000000000));
    }
}
