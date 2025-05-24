use std::{fmt, str::FromStr};

use gmsol_sdk::{constants::MARKET_TOKEN_DECIMALS, utils::decimal_to_amount};
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
    pub fn to_u64(&self) -> gmsol_sdk::Result<u64> {
        decimal_to_amount(self.0, LAMPORT_DECIMALS)
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
    pub fn to_u64(&self) -> gmsol_sdk::Result<u64> {
        decimal_to_amount(self.0, MARKET_TOKEN_DECIMALS)
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
    pub fn to_u64(&self, decimals: u8) -> gmsol_sdk::Result<u64> {
        decimal_to_amount(self.0, decimals)
    }
}
