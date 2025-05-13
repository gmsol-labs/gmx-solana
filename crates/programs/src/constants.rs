/// Default decimals for calculation.
pub const MARKET_DECIMALS: u8 = 20;

/// Unit USD value i.e. `one`.
pub const MARKET_USD_UNIT: u128 = 10u128.pow(MARKET_DECIMALS as u32);

/// Decimals of market tokens.
pub const MARKET_TOKEN_DECIMALS: u8 = 9;

/// USD value to amount divisor.
pub const MARKET_USD_TO_AMOUNT_DIVISOR: u128 =
    10u128.pow((MARKET_DECIMALS - MARKET_TOKEN_DECIMALS) as u32);

/// Adjustment factor for saving funding amount per size.
pub const FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT: u128 = 10u128.pow((MARKET_DECIMALS >> 1) as u32);

/// Number of market config flags.
pub const NUM_MARKET_CONFIG_FLAGS: usize = 128;

/// Number of market flags.
pub const NUM_MARKET_FLAGS: usize = 8;

/// Max length of the role anme.
pub const MAX_ROLE_NAME_LEN: usize = 32;
