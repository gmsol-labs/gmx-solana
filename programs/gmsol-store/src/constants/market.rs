use crate::states::Factor;

const SECONDS_PER_YEAR: Factor = 3600 * 24 * 365;

/// Default receiver factor.
pub const DEFAULT_RECEIVER_FACTOR: Factor = 70_000_000_000_000_000_000;

/// Default swap impact exponent.
pub const DEFAULT_SWAP_IMPACT_EXPONENT: Factor = super::MARKET_USD_UNIT;
/// Default swap impact positive factor.
pub const DEFAULT_SWAP_IMPACT_POSITIVE_FACTOR: Factor = 0;
/// Default swap impact negative factor.
pub const DEFAULT_SWAP_IMPACT_NEGATIVE_FACTOR: Factor = 0;

/// Default swap fee factor for positive impact.
pub const DEFAULT_SWAP_FEE_FACTOR_FOR_POSITIVE_IMPACT: Factor = 0;
/// Default swap fee factor for negative impact.
pub const DEFAULT_SWAP_FEE_FACTOR_FOR_NEGATIVE_IMPACT: Factor = 0;

/// Default min position size usd.
pub const DEFAULT_MIN_POSITION_SIZE_USD: Factor = super::MARKET_USD_UNIT;
/// Default min collateral value.
pub const DEFAULT_MIN_COLLATERAL_VALUE: Factor = super::MARKET_USD_UNIT;
/// Default min collateral factor.
pub const DEFAULT_MIN_COLLATERAL_FACTOR: Factor = super::MARKET_USD_UNIT / 100;
/// Default min collateral factor for open interest for long
pub const DEFAULT_MIN_COLLATERAL_FACTOR_FOR_OPEN_INTEREST_FOR_LONG: Factor = 380_000_000_000;
/// Default min collateral factor for open interest for short
pub const DEFAULT_MIN_COLLATERAL_FACTOR_FOR_OPEN_INTEREST_FOR_SHORT: Factor = 380_000_000_000;
/// Default max positive position impact factor.
pub const DEFAULT_MAX_POSITIVE_POSITION_IMPACT_FACTOR: Factor = 500_000_000_000_000_000;
/// Default max negative position impact factor.
pub const DEFAULT_MAX_NEGATIVE_POSITION_IMPACT_FACTOR: Factor = 500_000_000_000_000_000;
/// Default max position impact factor for liquidations.
pub const DEFAULT_MAX_POSITION_IMPACT_FACTOR_FOR_LIQUIDATIONS: Factor = 0;

/// Default position impact exponent.
pub const DEFAULT_POSITION_IMPACT_EXPONENT: Factor = 2 * super::MARKET_USD_UNIT;
/// Default position impact positive factor.
pub const DEFAULT_POSITION_IMPACT_POSITIVE_FACTOR: Factor = 10_000_000_000_000;
/// Default position impact negative factor.
pub const DEFAULT_POSITION_IMPACT_NEGATIVE_FACTOR: Factor = 20_000_000_000_000;

/// Default order fee factor for positive impact.
pub const DEFAULT_ORDER_FEE_FACTOR_FOR_POSITIVE_IMPACT: Factor = 50_000_000_000_000_000;
/// Default order fee factor for negative impact.
pub const DEFAULT_ORDER_FEE_FACTOR_FOR_NEGATIVE_IMPACT: Factor = 70_000_000_000_000_000;

/// Default position impact distribute factor.
pub const DEFAULT_POSITION_IMPACT_DISTRIBUTE_FACTOR: Factor = 230_000_000_000_000_000;
/// Default min position impact pool amount.
pub const DEFAULT_MIN_POSITION_IMPACT_POOL_AMOUNT: Factor = 1_500_000_000;

/// Default borrowing fee factor for long.
pub const DEFAULT_BORROWING_FEE_FACTOR_FOR_LONG: Factor = 2_780_000_000_000;
/// Default borrowing fee factor for short.
pub const DEFAULT_BORROWING_FEE_FACTOR_FOR_SHORT: Factor = 2_780_000_000_000;
/// Default borrowing fee exponent for long.
pub const DEFAULT_BORROWING_FEE_EXPONENT_FOR_LONG: Factor = super::MARKET_USD_UNIT;
/// Default borrowing fee exponent for short.
pub const DEFAULT_BORROWING_FEE_EXPONENT_FOR_SHORT: Factor = super::MARKET_USD_UNIT;
/// Default borrowing fee optimal usage factor for long.
pub const DEFAULT_BORROWING_FEE_OPTIMAL_USAGE_FACTOR_FOR_LONG: Factor =
    75 * super::MARKET_USD_UNIT / 100;
/// Default borrowing fee optimal usage factor for short.
pub const DEFAULT_BORROWING_FEE_OPTIMAL_USAGE_FACTOR_FOR_SHORT: Factor =
    75 * super::MARKET_USD_UNIT / 100;
/// Default borrowing fee base factor for long.
pub const DEFAULT_BORROWING_FEE_BASE_FACTOR_FOR_LONG: Factor =
    60 * super::MARKET_USD_UNIT / 100 / SECONDS_PER_YEAR;
/// Default borrowing fee base factor for short.
pub const DEFAULT_BORROWING_FEE_BASE_FACTOR_FOR_SHORT: Factor =
    60 * super::MARKET_USD_UNIT / 100 / SECONDS_PER_YEAR;
/// Default borrowing fee above optimal usage factor for long.
pub const DEFAULT_BORROWING_FEE_ABOVE_OPTIMAL_USAGE_FACTOR_FOR_LONG: Factor =
    150 * super::MARKET_USD_UNIT / 100 / SECONDS_PER_YEAR;
/// Default borrowing fee above optimal usage factor for short.
pub const DEFAULT_BORROWING_FEE_ABOVE_OPTIMAL_USAGE_FACTOR_FOR_SHORT: Factor =
    150 * super::MARKET_USD_UNIT / 100 / SECONDS_PER_YEAR;

/// Default funding fee exponent.
pub const DEFAULT_FUNDING_FEE_EXPONENT: Factor = super::MARKET_USD_UNIT;
/// Default funding factor.
pub const DEFAULT_FUNDING_FEE_FACTOR: Factor = 2_000_000_000_000;
/// Default funding fee max factor per second.
pub const DEFAULT_FUNDING_FEE_MAX_FACTOR_PER_SECOND: Factor = 1_500_000_000_000;
/// Default funding fee min factor per second.
pub const DEFAULT_FUNDING_FEE_MIN_FACTOR_PER_SECOND: Factor = 30_000_000_000;
/// Default funding fee increase factor per second.
pub const DEFAULT_FUNDING_FEE_INCREASE_FACTOR_PER_SECOND: Factor = 116_000_000;
/// Default funding fee decrease factor per second.
pub const DEFAULT_FUNDING_FEE_DECREASE_FACTOR_PER_SECOND: Factor = 0;
/// Default funding fee threshold for stable funding.
pub const DEFAULT_FUNDING_FEE_THRESHOLD_FOR_STABLE_FUNDING: Factor = 5_000_000_000_000_000_000;
/// Default funding fee threshold for decrease funding.
pub const DEFAULT_FUNDING_FEE_THRESHOLD_FOR_DECREASE_FUNDING: Factor = 0;

/// Default reserve factor.
pub const DEFAULT_RESERVE_FACTOR: Factor = 50_000_000_000_000_000_000;
/// Default open interest reserve factor.
pub const DEFAULT_OPEN_INTEREST_RESERVE_FACTOR: Factor = 45_000_000_000_000_000_000;

/// Default max pnl factor for long deposit.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_LONG_DEPOSIT: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for short deposit.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_DEPOSIT: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for long withdrawal.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_LONG_WITHDRAWAL: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for short withdrawal.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_WITHDRAWAL: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for long trader.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_LONG_TRADER: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for short trader.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_TRADER: Factor = 60_000_000_000_000_000_000;
/// Default max pnl factor for long adl.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_LONG_ADL: Factor = 55_000_000_000_000_000_000;
/// Default max pnl factor for short adl.
pub const DEFAULT_MAX_PNL_FACTOR_FOR_SHORT_ADL: Factor = 55_000_000_000_000_000_000;
/// Default min pnl factor after long adl.
pub const DEFAULT_MIN_PNL_FACTOR_AFTER_LONG_ADL: Factor = 50_000_000_000_000_000_000;
/// Default min pnl factor after short adl.
pub const DEFAULT_MIN_PNL_FACTOR_AFTER_SHORT_ADL: Factor = 50_000_000_000_000_000_000;

/// Default max pool amount for long token.
pub const DEFAULT_MAX_POOL_AMOUNT_FOR_LONG_TOKEN: Factor = 900_000_000_000;
/// Default max pool amount for short token.
pub const DEFAULT_MAX_POOL_AMOUNT_FOR_SHORT_TOKEN: Factor = 900_000_000_000;

/// Default max pool value for deposit for long token.
pub const DEFAULT_MAX_POOL_VALUE_FOR_DEPOSIT_LONG_TOKEN: Factor = 750_000 * super::MARKET_USD_UNIT;
/// Default max pool value for deposit for short token.
pub const DEFAULT_MAX_POOL_VALUE_FOR_DEPOSIT_SHORT_TOKEN: Factor = 750_000 * super::MARKET_USD_UNIT;

/// Default max open interest for long.
pub const DEFAULT_MAX_OPEN_INTEREST_FOR_LONG: Factor = 450_000 * super::MARKET_USD_UNIT;
/// Default max open interest for short.
pub const DEFAULT_MAX_OPEN_INTEREST_FOR_SHORT: Factor = 450_000 * super::MARKET_USD_UNIT;

/// Default min tokens for first deposit.
pub const DEFAULT_MIN_TOKENS_FOR_FIRST_DEPOSIT: Factor = 0;

/// Default skip borrowing fee for smaller side.
pub const DEFAULT_SKIP_BORROWING_FEE_FOR_SMALLER_SIDE: bool = true;

/// Default ignore open interest for usage factor.
pub const DEFAULT_IGNORE_OPEN_INTEREST_FOR_USAGE_FACTOR: bool = false;
