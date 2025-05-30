use std::{fmt, str::FromStr};

use crate::{
    constants::{MARKET_DECIMALS, MARKET_TOKEN_DECIMALS},
    utils::{decimal_to_amount, decimal_to_value},
};
use rust_decimal::Decimal;

use super::{
    decimal_to_signed_value, signed_value_to_decimal, unsigned_amount_to_decimal,
    unsigned_fixed_to_decimal, unsigned_value_to_decimal,
};

const LAMPORT_DECIMALS: u8 = 9;

/// Amount in lamports.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lamport(#[cfg_attr(serde, serde(with = "rust_decimal::serde::str"))] pub Decimal);

impl fmt::Display for Lamport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Lamport {
    type Err = <Decimal as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl Lamport {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Convert to `u64`
    pub fn to_u64(&self) -> crate::Result<u64> {
        decimal_to_amount(self.0, LAMPORT_DECIMALS)
    }

    /// Create from `u64`.
    pub fn from_u64(amount: u64) -> Self {
        Self(unsigned_amount_to_decimal(amount, LAMPORT_DECIMALS).normalize())
    }

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// Market token amount.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GmAmount(#[cfg_attr(serde, serde(with = "rust_decimal::serde::str"))] pub Decimal);

impl fmt::Display for GmAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for GmAmount {
    type Err = <Decimal as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl GmAmount {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Convert to `u64`
    pub fn to_u64(&self) -> crate::Result<u64> {
        decimal_to_amount(self.0, MARKET_TOKEN_DECIMALS)
    }

    /// Create from `u64`.
    pub fn from_u64(amount: u64) -> Self {
        Self(unsigned_amount_to_decimal(amount, MARKET_TOKEN_DECIMALS).normalize())
    }

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// A general-purpose token amount.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(#[cfg_attr(serde, serde(with = "rust_decimal::serde::str"))] pub Decimal);

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Amount {
    type Err = <Decimal as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl Amount {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Convert to `u64`.
    pub fn to_u64(&self, decimals: u8) -> crate::Result<u64> {
        decimal_to_amount(self.0, decimals)
    }

    /// Create from `u64`.
    pub fn from_u64(amount: u64, decimals: u8) -> Self {
        Self(unsigned_amount_to_decimal(amount, decimals).normalize())
    }

    /// Convert to `u128`.
    pub fn to_u128(&self, decimals: u8) -> crate::Result<u128> {
        decimal_to_value(self.0, decimals)
    }

    /// Create from `u128`.
    pub fn from_u128(amount: u128, decimals: u8) -> crate::Result<Self> {
        Ok(Self(
            unsigned_fixed_to_decimal(amount, decimals)
                .ok_or_else(|| crate::Error::custom("amount exceeds the maximum value"))?
                .normalize(),
        ))
    }

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// A value with [`MARKET_DECIMALS`] decimals.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Value(#[cfg_attr(serde, serde(with = "rust_decimal::serde::str"))] pub Decimal);

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Value {
    type Err = <Decimal as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl Value {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Convert to `u128`.
    pub fn to_u128(&self) -> crate::Result<u128> {
        decimal_to_value(self.0, MARKET_DECIMALS)
    }

    /// Convert to `i128`.
    pub fn to_i128(&self) -> i128 {
        decimal_to_signed_value(self.0, MARKET_DECIMALS)
    }

    /// Create from `i128`.
    pub fn from_i128(value: i128) -> Self {
        Self(signed_value_to_decimal(value).normalize())
    }

    /// Create from `u128`.
    pub fn from_u128(value: u128) -> Self {
        Self(unsigned_value_to_decimal(value).normalize())
    }

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}
