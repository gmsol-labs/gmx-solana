use std::fmt;

use num_traits::{
    CheckedAdd, CheckedDiv, CheckedMul, CheckedNeg, CheckedSub, FromPrimitive, One, Signed,
};

/// Num trait used in GMX.
pub trait Num:
    num_traits::Num
    + CheckedAdd
    + CheckedMul
    + CheckedSub
    + CheckedNeg
    + CheckedDiv
    + Clone
    + Ord
    + FromPrimitive
    + fmt::Debug
    + fmt::Display
{
}

impl<
        T: num_traits::Num
            + CheckedAdd
            + CheckedMul
            + CheckedSub
            + CheckedNeg
            + CheckedDiv
            + Clone
            + Ord
            + FromPrimitive
            + fmt::Debug
            + fmt::Display,
    > Num for T
{
}

/// Unsigned value that cannot be negative.
pub trait Unsigned: num_traits::Unsigned {
    /// The signed type.
    type Signed: TryFrom<Self> + UnsignedAbs<Unsigned = Self> + CheckedNeg;

    /// Convert to a signed value
    fn to_signed(&self) -> crate::Result<Self::Signed>
    where
        Self: Clone,
    {
        self.clone().try_into().map_err(|_| crate::Error::Convert)
    }

    /// Convert to a signed value with the given sign.
    fn to_signed_with_sign(&self, negative: bool) -> crate::Result<Self::Signed>
    where
        Self: Clone,
        Self::Signed: CheckedSub,
    {
        if negative {
            self.to_opposite_signed()
        } else {
            self.to_signed()
        }
    }

    /// Convert to opposite signed.
    fn to_opposite_signed(&self) -> crate::Result<Self::Signed>
    where
        Self: Clone,
        Self::Signed: CheckedSub,
    {
        self.to_signed()?
            .checked_neg()
            .ok_or(crate::Error::Computation("to opposite signed"))
    }

    /// Compute the absolute difference of two values.
    fn diff(self, other: Self) -> Self;

    /// Compute signed `self - other`.
    fn checked_signed_sub(self, other: Self) -> crate::Result<Self::Signed>
    where
        Self: Ord + Clone,
        Self::Signed: CheckedSub,
    {
        if self >= other {
            self.diff(other).to_signed()
        } else {
            self.diff(other).to_opposite_signed()
        }
    }

    /// Checked signed add.
    fn checked_add_with_signed(&self, other: &Self::Signed) -> Option<Self>
    where
        Self: CheckedAdd + CheckedSub,
    {
        let value = other.unsigned_abs();
        if other.is_positive() {
            self.checked_add(&value)
        } else {
            self.checked_sub(&value)
        }
    }

    /// Checked signed sub.
    fn checked_sub_with_signed(&self, other: &Self::Signed) -> Option<Self>
    where
        Self: CheckedAdd + CheckedSub,
    {
        let value = other.unsigned_abs();
        if other.is_positive() {
            self.checked_sub(&value)
        } else {
            self.checked_add(&value)
        }
    }

    /// Checked signed mul.
    fn checked_mul_with_signed(&self, other: &Self::Signed) -> Option<Self::Signed>
    where
        Self: CheckedMul,
    {
        let value = other.unsigned_abs();
        if other.is_negative() {
            Some(
                Self::Signed::try_from(self.checked_mul(&value)?)
                    .ok()?
                    .checked_neg()?,
            )
        } else {
            self.checked_mul(&value)?.try_into().ok()
        }
    }

    /// As divisor to checked divide other and round up magnitude.
    fn as_divisor_to_round_up_magnitude_div(&self, dividend: &Self::Signed) -> Option<Self::Signed>
    where
        Self: Clone,
        Self::Signed: CheckedSub + CheckedAdd + CheckedDiv,
    {
        if self.is_zero() {
            return None;
        }
        let divisor: Self::Signed = self.clone().try_into().ok()?;
        if dividend.is_negative() {
            dividend
                .checked_sub(&divisor)?
                .checked_add(&One::one())?
                .checked_div(&divisor)
        } else {
            dividend
                .checked_add(&divisor)?
                .checked_sub(&One::one())?
                .checked_div(&divisor)
        }
    }

    /// Checked round up division.
    fn checked_round_up_div(&self, divisor: &Self) -> Option<Self>
    where
        Self: CheckedAdd + CheckedSub + Clone + CheckedDiv,
    {
        if divisor.is_zero() {
            return None;
        }
        self.checked_add(divisor)?
            .checked_sub(&One::one())?
            .checked_div(divisor)
    }

    /// Bound the magnitude of a signed value.
    ///
    /// # Errors
    /// Return error if
    /// - `min > max`
    /// - `min` is greater than the maximum value representable by `Self::Signed`
    ///
    /// # Examples
    ///
    /// This method can be used to bound the magnitude of a signed value:
    /// ```
    /// # use gmsol_model::num::Unsigned;
    /// let a = -123i64;
    /// // Original value within bounds
    /// assert_eq!(Unsigned::bound_magnitude(&a, &0, &124u64).unwrap(), -123);
    /// // Value clamped to max magnitude
    /// assert_eq!(Unsigned::bound_magnitude(&a, &0, &120u64).unwrap(), -120);
    /// // Value clamped to min magnitude
    /// assert_eq!(Unsigned::bound_magnitude(&a, &124, &256u64).unwrap(), -124);
    ///
    /// let b = 123i64;
    /// // Original value within bounds
    /// assert_eq!(Unsigned::bound_magnitude(&b, &0, &124u64).unwrap(), 123);
    /// // Value clamped to max magnitude
    /// assert_eq!(Unsigned::bound_magnitude(&b, &0, &120u64).unwrap(), 120);
    /// // Value clamped to min magnitude
    /// assert_eq!(Unsigned::bound_magnitude(&b, &124, &256u64).unwrap(), 124);
    /// ```
    ///
    /// Returns an error if `min > max`:
    /// ```
    /// # use gmsol_model::num::Unsigned;
    /// let result = Unsigned::bound_magnitude(&0, &1u64, &0);
    /// assert!(result.is_err());
    /// ```
    ///
    /// Returns an error if `min` is greater than the maximum value representable by `Self::Signed`:
    /// ```
    /// # use gmsol_model::num::Unsigned;
    /// let result = Unsigned::bound_magnitude(&0, &(u64::MAX / 2 + 1), &u64::MAX);
    /// assert!(result.is_err());
    /// ```
    fn bound_magnitude(value: &Self::Signed, min: &Self, max: &Self) -> crate::Result<Self::Signed>
    where
        Self: Ord + Clone,
        Self::Signed: Clone + CheckedSub,
    {
        if min > max {
            return Err(crate::Error::InvalidArgument("min > max"));
        }
        let magnitude = value.unsigned_abs();
        let negative = value.is_negative();
        if magnitude < *min {
            min.to_signed_with_sign(negative)
        } else if magnitude > *max {
            max.to_signed_with_sign(negative)
        } else {
            Ok(value.clone())
        }
    }
}

/// Convert signed value to unsigned.
pub trait UnsignedAbs: Signed {
    /// Unsigned type.
    type Unsigned;

    /// Computes the absolute value and returns as a unsigned value.
    fn unsigned_abs(&self) -> Self::Unsigned;
}

/// Perform Mul-Div calculation with bigger range num type.
pub trait MulDiv: Unsigned {
    /// Calculates floor(self * numerator / denominator) with full precision.
    ///
    /// Returns `None` if the `denominator` is zero or overflow.
    fn checked_mul_div(&self, numerator: &Self, denominator: &Self) -> Option<Self>;

    /// Calculates ceil(self * numerator / denominator) with full precision.
    ///
    /// Returns `None` if the `denominator` is zero or overflow.
    fn checked_mul_div_ceil(&self, numerator: &Self, denominator: &Self) -> Option<Self>;

    /// Calculates floor(self * numerator / denominator) with full precision,
    /// where `numerator` is signed.
    ///
    /// Returns `None` if the `denominator` is zero or overflow.
    fn checked_mul_div_with_signed_numerator(
        &self,
        numerator: &Self::Signed,
        denominator: &Self,
    ) -> Option<Self::Signed> {
        let ans = self
            .checked_mul_div(&numerator.unsigned_abs(), denominator)?
            .try_into()
            .ok()?;
        if numerator.is_positive() {
            Some(ans)
        } else {
            ans.checked_neg()
        }
    }
}

impl Unsigned for u64 {
    type Signed = i64;

    fn diff(self, other: Self) -> Self {
        self.abs_diff(other)
    }
}

impl MulDiv for u64 {
    #[allow(clippy::arithmetic_side_effects)]
    fn checked_mul_div(&self, numerator: &Self, denominator: &Self) -> Option<Self> {
        if *denominator == 0 {
            return None;
        }
        let x = *self as u128;
        let numerator = *numerator as u128;
        let denominator = *denominator as u128;
        let ans = x * numerator / denominator;
        ans.try_into().ok()
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn checked_mul_div_ceil(&self, numerator: &Self, denominator: &Self) -> Option<Self> {
        if *denominator == 0 {
            return None;
        }
        let x = *self as u128;
        let numerator = *numerator as u128;
        let denominator = *denominator as u128;
        let ans = (x * numerator + denominator - 1) / denominator;
        ans.try_into().ok()
    }
}

impl UnsignedAbs for i64 {
    type Unsigned = u64;

    fn unsigned_abs(&self) -> u64 {
        (*self).unsigned_abs()
    }
}

#[cfg(feature = "u128")]
/// Add support to `u128`.
mod u128 {
    use super::{MulDiv, Unsigned, UnsignedAbs};
    use ruint::aliases::U256;

    impl Unsigned for u128 {
        type Signed = i128;

        fn diff(self, other: Self) -> Self {
            self.abs_diff(other)
        }
    }

    impl UnsignedAbs for i128 {
        type Unsigned = u128;

        fn unsigned_abs(&self) -> u128 {
            (*self).unsigned_abs()
        }
    }

    impl MulDiv for u128 {
        #[allow(clippy::arithmetic_side_effects)]
        fn checked_mul_div(&self, numerator: &Self, denominator: &Self) -> Option<Self> {
            if *denominator == 0 {
                return None;
            }
            let x = U256::from(*self);
            let numerator = U256::from(*numerator);
            let denominator = U256::from(*denominator);
            let ans = x * numerator / denominator;
            ans.try_into().ok()
        }

        #[allow(clippy::arithmetic_side_effects)]
        fn checked_mul_div_ceil(&self, numerator: &Self, denominator: &Self) -> Option<Self> {
            if *denominator == 0 {
                return None;
            }
            let x = U256::from(*self);
            let numerator = U256::from(*numerator);
            let denominator = U256::from(*denominator);
            let ans = (x * numerator).div_ceil(denominator);
            ans.try_into().ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_up_magnitude_division() {
        let b = 3u64;
        let positive = 1i64;
        let negative = -1i64;

        assert_eq!(b.as_divisor_to_round_up_magnitude_div(&positive), Some(1));
        assert_eq!(b.as_divisor_to_round_up_magnitude_div(&negative), Some(-1));
    }

    #[test]
    fn round_up_division() {
        let b = 3u64;
        let a = 1u64;
        assert_eq!(a.checked_round_up_div(&b), Some(1));
    }

    #[test]
    fn mul_div_ceil() {
        let a = 650_406_504u64;
        let a2 = 650_406_505u64;
        let b = 40_000_000_000u64;
        let c = 80_000_000_000u64;
        assert_eq!(a.checked_mul_div_ceil(&b, &c).unwrap(), 325_203_252);
        assert_eq!(a2.checked_mul_div_ceil(&b, &c).unwrap(), 325_203_253);
    }

    #[cfg(feature = "u128")]
    #[test]
    fn mul_div_ceil_u128() {
        let a = 650_406_504u128;
        let a2 = 650_406_505u128;
        let b = 40_000_000_000u128;
        let c = 80_000_000_000u128;
        assert_eq!(a.checked_mul_div_ceil(&b, &c).unwrap(), 325_203_252);
        assert_eq!(a2.checked_mul_div_ceil(&b, &c).unwrap(), 325_203_253);
    }

    #[test]
    fn bound_magnitude() {
        let a = -123i64;
        assert_eq!(Unsigned::bound_magnitude(&a, &0, &124u64).unwrap(), -123);
        assert_eq!(Unsigned::bound_magnitude(&a, &0, &120u64).unwrap(), -120);
        assert_eq!(Unsigned::bound_magnitude(&a, &124, &256u64).unwrap(), -124);
        assert_eq!(Unsigned::bound_magnitude(&a, &125, &125u64).unwrap(), -125);

        let b = 123i64;
        assert_eq!(Unsigned::bound_magnitude(&b, &0, &124u64).unwrap(), 123);
        assert_eq!(Unsigned::bound_magnitude(&b, &0, &120u64).unwrap(), 120);
        assert_eq!(Unsigned::bound_magnitude(&b, &124, &256u64).unwrap(), 124);
        assert_eq!(Unsigned::bound_magnitude(&b, &125, &125u64).unwrap(), 125);

        let c = 0i64;
        assert_eq!(Unsigned::bound_magnitude(&c, &1, &124u64).unwrap(), 1);

        let d = -0i64;
        assert_eq!(Unsigned::bound_magnitude(&d, &1, &124u64).unwrap(), 1);

        let result = Unsigned::bound_magnitude(&0, &u64::MAX, &u64::MAX);
        assert!(result.is_err());
    }
}
