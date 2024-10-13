use std::cmp::Ordering;

use anchor_lang::{
    prelude::{borsh, AnchorDeserialize, AnchorSerialize},
    InitSpace,
};

/// Decimal type for storing prices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AnchorSerialize, AnchorDeserialize, InitSpace)]
pub struct Decimal {
    /// Value.
    pub value: u32,
    /// Decimal multiplier.
    pub decimal_multiplier: u8,
}

impl Decimal {
    /// The Maximum Decimals.
    /// Should satisfy `MAX_DECIMALS <= 30`.
    pub const MAX_DECIMALS: u8 = 20;

    /// The Maximum Decimal Multiplier,
    /// which should satisfy `u32::MAX * 10^{MAX_DECIMAL_MULTIPLIER} <= u128::MAX`.
    pub const MAX_DECIMAL_MULTIPLIER: u8 = 20;

    /// Returns the price of one unit (with decimals to be [`MAX_DECIMALS`](Self::MAX_DECIMALS)).
    pub fn to_unit_price(&self) -> u128 {
        self.value as u128 * 10u128.pow(self.decimal_multiplier as u32)
    }

    /// Create price decimal from the given `price` with `decimals`,
    /// where `token_decimals` is the expected unit and with expected `precision`.
    pub fn try_from_price(
        mut price: u128,
        decimals: u8,
        token_decimals: u8,
        precision: u8,
    ) -> Result<Self, DecimalError> {
        if token_decimals > Self::MAX_DECIMALS
            || precision > Self::MAX_DECIMALS
            || decimals > Self::MAX_DECIMALS
        {
            return Err(DecimalError::ExceedMaxDecimals);
        }
        if token_decimals + precision > Self::MAX_DECIMALS {
            return Err(DecimalError::ExceedMaxDecimals);
        }
        // Convert the `price` to be with decimals of `token_decimals`.
        let divisor_exp = match decimals.cmp(&token_decimals) {
            Ordering::Equal => None,
            Ordering::Less => {
                // CHECK: Since `token_decimals` and `decimals` are both less than `MAX_DECIMALS`,
                // the pow will never overflow.
                let multiplier = 10u128.pow((token_decimals - decimals) as u32);
                price = price
                    .checked_mul(multiplier)
                    .ok_or(DecimalError::Overflow)?;
                None
            }
            Ordering::Greater => Some(decimals - token_decimals),
        };

        let decimal_multiplier = Self::decimal_multiplier_from_precision(token_decimals, precision);
        debug_assert!(
            decimal_multiplier <= Self::MAX_DECIMAL_MULTIPLIER,
            "must not exceed `MAX_DECIMAL_MULTIPLIER`"
        );
        // CHECK: 2 * MAX_DECIMALS + MAX_DECIMAL_MULTIPLER <= u8::MAX
        let multiplier = (token_decimals << 1) + decimal_multiplier;
        let value = if Self::MAX_DECIMALS >= multiplier {
            let mut exp = Self::MAX_DECIMALS - multiplier;
            if let Some(divisor_exp) = divisor_exp {
                if exp >= divisor_exp {
                    exp -= divisor_exp;
                    // CHECK: Since `exp <= MAX_DECIMALS <= 30`, the pow will never overflow.
                    price
                        .checked_mul(10u128.pow((exp) as u32))
                        .ok_or(DecimalError::Overflow)?
                } else {
                    exp = divisor_exp - exp;
                    // CHECK: Since `divisor_exp <= decimals <= MAX_DECIMALS <= 30`, the pow will never overflow.
                    price / 10u128.pow(exp as u32)
                }
            } else {
                // CHECK: Since `exp <= MAX_DECIMALS <= 30`, the pow will never overflow.
                price
                    .checked_mul(10u128.pow((exp) as u32))
                    .ok_or(DecimalError::Overflow)?
            }
        } else {
            // CHECK: Since `multiplier == 2 * token_decimals + decimal_multiplier <= token_decimals + MAX_DECIMALS <= 2 * MAX_DECIMALS`,
            // `multiplier - MAX_DECIMALS <= MAX_DECIMALS <= 30` will never make the pow overflow.
            let mut ans = price / 10u128.pow((multiplier - Self::MAX_DECIMALS) as u32);
            if let Some(exp) = divisor_exp {
                ans /= 10u128.pow(exp as u32)
            }
            ans
        };
        Ok(Self {
            value: value as u32,
            decimal_multiplier,
        })
    }

    /// Calculate the decimal multiplier with the desired precision.
    /// # Warning
    /// One should check that `decimals + precision` is not greater than [`MAX_DECIMALS`](Self::MAX_DECIMALS),
    /// otherwise the result might be incorrect due to underflow.
    pub const fn decimal_multiplier_from_precision(decimals: u8, precision: u8) -> u8 {
        Self::MAX_DECIMALS - decimals - precision
    }

    /// Returns the max representable decimal with the same decimal multiplier.
    pub fn maximum(&self) -> Self {
        Self {
            value: u32::MAX,
            decimal_multiplier: self.decimal_multiplier,
        }
    }
}

/// Errors of decimals.
#[derive(Debug, thiserror::Error)]
pub enum DecimalError {
    /// Exceed the maximum decimals.
    #[error("exceeds the maximum decimals")]
    ExceedMaxDecimals,
    /// Invalid decimals.
    #[error("exceeds the maximum decimal multipler")]
    ExceedMaxDecimalMultipler,
    /// Overflow.
    #[error("overflow")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_1() {
        // The price of ETH is 5,000 with 18 decimals and the decimal multipler is set to 8 (so that we have decimals of precision 4).
        let price = Decimal::try_from_price(5_000_000_000_000_000_000_000, 18, 8, 4).unwrap();
        assert_eq!(price.to_unit_price(), 5_000_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 8);
    }

    #[test]
    fn test_price_2() {
        // The price of BTC is 60,000 with 8 decimals and the decimal multipler is set to 10 (so that we have decimals of precision 2).
        let price = Decimal::try_from_price(6_000_000_000_000, 8, 8, 2).unwrap();
        assert_eq!(price.to_unit_price(), 60_000_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 10);
    }

    #[test]
    fn test_price_3() {
        // The price of USDC is 1 with 6 decimals and the decimal multipler is set to 8 (so that we have decimals of precision 6).
        let price = Decimal::try_from_price(1_000_000, 6, 6, 6).unwrap();
        assert_eq!(price.to_unit_price(), 100_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 8);
    }

    #[test]
    fn test_price_4() {
        // The price of DG is 0.00000001 with 18 decimals and the decimal multipler is set to 1 (so that we have decimals of precision 11).
        let price = Decimal::try_from_price(10_000_000_000, 18, 8, 11).unwrap();
        assert_eq!(price.to_unit_price(), 10_000);
        assert_eq!(price.decimal_multiplier, 1);
    }

    #[test]
    fn test_price_5() {
        // The price of one WNT is 5,000
        // price decimals: 5
        // token decimals: 8
        // expected precision: 4
        let price = Decimal::try_from_price(500_000_000, 5, 8, 4).unwrap();
        // 5,000 / 10^18 * 10^20
        assert_eq!(price.to_unit_price(), 5_000_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 20 - 8 - 4);
    }

    #[test]
    fn test_price_6() {
        // The price of one WBTC is 50,000
        // price decimals: 8
        // token decimals: 8
        // expected precision: 2
        let price = Decimal::try_from_price(5_000_000_000_000, 8, 8, 2).unwrap();
        // 50,000 / 10^8 * 10^20
        assert_eq!(price.to_unit_price(), 50_000_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 20 - 8 - 2);
    }

    #[test]
    fn test_price_7() {
        // The price of one token is 5.0
        // price decimals: 12
        // token decimals: 8
        // expected precision: 2
        let price = Decimal::try_from_price(5_000_000_000_000, 12, 8, 2).unwrap();
        // 5 / 10^8 * 10^20
        assert_eq!(price.to_unit_price(), 5_000_000_000_000);
        assert_eq!(price.decimal_multiplier, 20 - 8 - 2);
    }

    #[test]
    fn test_price_8() {
        // The price of one SHIB is 1.77347 * 10^-5
        // price decimals: 10
        // token decimals: 5
        // expected precision: 9
        let price = Decimal::try_from_price(177_347, 10, 5, 9).unwrap();
        assert_eq!(price.to_unit_price(), 17_734_000_000);
        assert_eq!(price.decimal_multiplier, 20 - 5 - 9);
    }
}
