use num_traits::{Signed, Zero};

use crate::{
    num::{Unsigned, UnsignedAbs},
    price::Prices,
    utils, LiquidityMarket, LiquidityMarketExt, PnlFactorKind,
};

/// Represents the value of the given market token amount in GLV
pub struct GlvValueForMarket<T: Unsigned> {
    /// The value of the given market token amount in GLV
    pub market_token_value_in_glv: T,
    /// The pool value for the given market.
    pub pool_value: T::Signed,
    /// The supply of the given market token.
    pub supply: T,
}

impl<T: Unsigned> GlvValueForMarket<T> {
    /// Create from parts.
    pub fn new(glv_value: T, pool_value: T::Signed, supply: T) -> Self {
        Self {
            market_token_value_in_glv: glv_value,
            pool_value,
            supply,
        }
    }
}

/// Returns the value of the given market token amount for GLV pricing.
pub fn get_glv_value_for_market<M, const DECIMALS: u8>(
    prices: &Prices<M::Num>,
    market: &M,
    balance: M::Num,
    maximize: bool,
) -> crate::Result<GlvValueForMarket<M::Num>>
where
    M: LiquidityMarket<DECIMALS>,
{
    let value = market.pool_value(prices, PnlFactorKind::MaxAfterDeposit, maximize)?;

    let supply = market.total_supply();

    if balance.is_zero() {
        return Ok(GlvValueForMarket::new(Zero::zero(), value, supply));
    }

    if value.is_negative() {
        return Err(crate::Error::InvalidPoolValue(
            crate::error::GLV_PRICING_NEGATIVE_POOL_VALUE_ERROR,
        ));
    }

    let glv_value = utils::market_token_amount_to_usd(&balance, &value.unsigned_abs(), &supply)
        .ok_or(crate::Error::Computation(
            crate::error::GLV_PRICING_MARKET_TOKEN_TO_GLV_VALUE_ERROR,
        ))?;

    Ok(GlvValueForMarket::new(glv_value, value, supply))
}

/// Converts the given GLV value to market token amount.
pub fn get_market_token_amount_for_glv_value<M, const DECIMALS: u8>(
    prices: &Prices<M::Num>,
    market: &M,
    glv_value: M::Num,
    maximize: bool,
    glv_value_to_amount_divisor: M::Num,
) -> crate::Result<M::Num>
where
    M: LiquidityMarket<DECIMALS>,
{
    let value = market.pool_value(prices, PnlFactorKind::MaxAfterWithdrawal, maximize)?;

    if value.is_negative() {
        return Err(crate::Error::InvalidPoolValue(
            crate::error::GLV_PRICING_NEGATIVE_POOL_VALUE_ERROR,
        ));
    }

    let supply = market.total_supply();

    let market_token_amount = utils::usd_to_market_token_amount(
        glv_value,
        value.unsigned_abs(),
        supply,
        glv_value_to_amount_divisor,
    )
    .ok_or(crate::Error::Computation(
        crate::error::GLV_PRICING_GLV_VALUE_TO_MARKET_TOKEN_ERROR,
    ))?;

    Ok(market_token_amount)
}
