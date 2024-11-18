/// Default market configs.
pub mod market;

pub use self::market::*;

use gmsol_utils::price::Decimal;

use crate::states::{Amount, Factor};

/// Event authority SEED.
pub const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

/// Market Token Mint Address Seed.
pub const MAREKT_TOKEN_MINT_SEED: &[u8] = b"market_token_mint";

/// Market Vault Seed.
pub const MARKET_VAULT_SEED: &[u8] = b"market_vault";

/// GT Mint Seed.
pub const GT_MINT_SEED: &[u8] = b"gt";

/// Claimable Account Seed.
pub const CLAIMABLE_ACCOUNT_SEED: &[u8] = b"claimable_account";

/// Escrow Account Seed.
pub const ESCROW_ACCOUNT_SEED: &[u8] = b"escrow_account";

/// Decimals of a market token.
pub const MARKET_TOKEN_DECIMALS: u8 = 9;

/// Unit USD value i.e. `one`.
pub const MARKET_USD_UNIT: u128 = 10u128.pow(MARKET_DECIMALS as u32);

/// Adjustment factor for saving funding amount per size.
pub const FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT: u128 = 10u128.pow((MARKET_DECIMALS >> 1) as u32);

/// USD value to amount divisor.
pub const MARKET_USD_TO_AMOUNT_DIVISOR: u128 =
    10u128.pow((MARKET_DECIMALS - MARKET_TOKEN_DECIMALS) as u32);

/// Decimals of usd values of factors.
pub const MARKET_DECIMALS: u8 = Decimal::MAX_DECIMALS;

/// Default claimable time window.
pub const DEFAULT_CLAIMABLE_TIME_WINDOW: Amount = 3600;

/// Default recent time window.
pub const DEFAULT_RECENT_TIME_WINDOW: Amount = 300;

/// Default request expiration.
pub const DEFAULT_REQUEST_EXPIRATION: Amount = 3600;

/// Default oracle max age.
pub const DEFAULT_ORACLE_MAX_AGE: Amount = 3600;

/// Default oracle max timestamp range.
pub const DEFAULT_ORACLE_MAX_TIMESTAMP_RANGE: Amount = 300;

/// Default oracle ref price deviation.
pub const DEFAULT_ORACLE_REF_PRICE_DEVIATION: Factor = 1_000_000_000_000_000;

/// Default GT vault time window size.
pub const DEFAULT_GT_VAULT_TIME_WINDOW: u32 = 24 * 60 * 60;

/// Default GT reserve factor.
pub const DEFAULT_GT_RESERVE_FACTOR: u128 = 5 * MARKET_USD_UNIT;

/// Default esGT vesting divisor.
pub const DEFAULT_ES_GT_VESTING_DIVISOR: u16 = 365;
