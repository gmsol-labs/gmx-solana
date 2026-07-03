/// Roles.
pub mod roles;

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

/// Error message indicating the virtual inventory for swaps is required but missing.
pub const VI_FOR_SWAPS_MISSING_ERROR: &str =
    "virtual inventory for swaps should be present but is missing";

/// Error message indicating the virtual inventory for swaps is provided but not expected.
pub const VI_FOR_SWAPS_UNEXPECTED_ERROR: &str =
    "virtual inventory for swaps should not be present but is provided";

/// Error message indicating the virtual inventory for positions is required but missing.
pub const VI_FOR_POSITIONS_MISSING_ERROR: &str =
    "virtual inventory for positions should be present but is missing";

/// Error message indicating the virtual inventory for positions is provided but not expected.
pub const VI_FOR_POSITIONS_UNEXPECTED_ERROR: &str =
    "virtual inventory for positions should not be present but is provided";
