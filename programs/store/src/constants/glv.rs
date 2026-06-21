use super::MARKET_USD_UNIT;

/// Default GLV min shift interval seconds.
pub const DEFAULT_GLV_MIN_SHIFT_INTERVAL_SECS: u32 = 60 * 60;

/// Default GLV max shift price impact factor.
pub const DEFAULT_GLV_MAX_SHIFT_PRICE_IMPACT_FACTOR: u128 = MARKET_USD_UNIT / 100;

/// Default GLV min shift value.
pub const DEFAULT_GLV_MIN_SHIFT_VALUE: u128 = 0;

/// Default GLV min tokens for first deposit.
pub const DEFAULT_GLV_MIN_TOKENS_FOR_FIRST_DEPOSIT: u64 =
    10u64.pow(super::MARKET_TOKEN_DECIMALS as u32);
