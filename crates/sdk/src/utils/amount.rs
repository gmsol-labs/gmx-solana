use std::{fmt, str::FromStr};

use crate::{
    constants::{MARKET_DECIMALS, MARKET_TOKEN_DECIMALS},
    utils::{decimal_to_amount, decimal_to_value},
};
use rust_decimal::Decimal;

const LAMPORT_DECIMALS: u8 = 9;

/// Amount in lamports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lamport(pub Decimal);

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

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// Market token amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GmAmount(pub Decimal);

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

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// A general-purpose token amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(pub Decimal);

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

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

/// A USD value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UsdValue(pub Decimal);

impl fmt::Display for UsdValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UsdValue {
    type Err = <Decimal as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl UsdValue {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Convert to `u128`.
    pub fn to_u128(&self) -> crate::Result<u128> {
        decimal_to_value(self.0, MARKET_DECIMALS)
    }

    /// Returns whether the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}
