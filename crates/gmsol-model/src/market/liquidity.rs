use num_traits::{CheckedAdd, CheckedDiv, CheckedMul, CheckedSub, Signed, Zero};

use crate::{
    action::{deposit::Deposit, withdraw::Withdrawal},
    fixed::FixedPointOps,
    market::utils::MarketUtils,
    num::{Unsigned, UnsignedAbs},
    price::Prices,
    BorrowingFeeMarket, PnlFactorKind, PositionImpactMarket,
};

use super::{
    get_msg_by_side, BaseMarketExt, BorrowingFeeMarketExt, PositionImpactMarketExt, SwapMarketMut,
};

/// A market for providing liquidity.
pub trait LiquidityMarket<const DECIMALS: u8>:
    PositionImpactMarket<DECIMALS> + BorrowingFeeMarket<DECIMALS>
{
    /// Get total supply of the market token.
    fn total_supply(&self) -> Self::Num;

    /// Get max pool value for deposit.
    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> crate::Result<Self::Num>;
}

/// A market for providing liquidity.
pub trait LiquidityMarketMut<const DECIMALS: u8>:
    SwapMarketMut<DECIMALS> + LiquidityMarket<DECIMALS>
{
    /// Perform mint.
    fn mint(&mut self, amount: &Self::Num) -> Result<(), crate::Error>;

    /// Perform burn.
    fn burn(&mut self, amount: &Self::Num) -> crate::Result<()>;
}

impl<M: LiquidityMarket<DECIMALS>, const DECIMALS: u8> LiquidityMarket<DECIMALS> for &mut M {
    fn total_supply(&self) -> Self::Num {
        (**self).total_supply()
    }

    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> crate::Result<Self::Num> {
        (**self).max_pool_value_for_deposit(is_long_token)
    }
}

impl<M: LiquidityMarketMut<DECIMALS>, const DECIMALS: u8> LiquidityMarketMut<DECIMALS> for &mut M {
    fn mint(&mut self, amount: &Self::Num) -> Result<(), crate::Error> {
        (**self).mint(amount)
    }

    fn burn(&mut self, amount: &Self::Num) -> crate::Result<()> {
        (**self).burn(amount)
    }
}

/// Extension trait of [`LiquidityMarket`].
pub trait LiquidityMarketExt<const DECIMALS: u8>: LiquidityMarket<DECIMALS> {
    /// Validate (primary) pool value for deposit.
    fn validate_pool_value_for_deposit(
        &self,
        prices: &Prices<Self::Num>,
        is_long_token: bool,
    ) -> crate::Result<()> {
        let pool_value = self.pool_value_without_pnl_for_one_side(prices, is_long_token, true)?;
        let max_pool_value = self.max_pool_value_for_deposit(is_long_token)?;
        if pool_value > max_pool_value {
            Err(crate::Error::MaxPoolValueExceeded(get_msg_by_side(
                is_long_token,
            )))
        } else {
            Ok(())
        }
    }

    /// Get the usd value of primary pool.
    fn pool_value(
        &self,
        prices: &Prices<Self::Num>,
        pnl_factor: PnlFactorKind,
        maximize: bool,
    ) -> crate::Result<Self::Signed> {
        let long_value = self.pool_value_without_pnl_for_one_side(prices, true, maximize)?;
        let short_value = self.pool_value_without_pnl_for_one_side(prices, false, maximize)?;

        let mut pool_value = long_value
            .checked_add(&short_value)
            .ok_or(crate::Error::Overflow)?
            .to_signed()?;

        // Add total pending borrowing fees.
        let total_borrowing_fees = {
            let for_long = self.total_pending_borrowing_fees(prices, true)?;
            let for_short = self.total_pending_borrowing_fees(prices, false)?;
            for_long
                .checked_add(&for_short)
                .ok_or(crate::Error::Computation(
                    "calculating total pending borrowing fees for pool value",
                ))?
        };
        let total_borrowing_fees_for_pool = <Self::Num>::UNIT
            .checked_sub(self.borrowing_fee_params()?.receiver_factor())
            .and_then(|factor| crate::utils::apply_factor(&total_borrowing_fees, &factor))
            .ok_or(crate::Error::Computation(
                "calculating total borrowing fees for pool",
            ))?
            .to_signed()?;
        pool_value = pool_value
            .checked_add(&total_borrowing_fees_for_pool)
            .ok_or(crate::Error::Computation(
                "adding total borrowing fees for pool",
            ))?;

        // Deduct net pnl.
        let long_pnl = {
            let pnl = self.pnl(&prices.index_token_price, true, !maximize)?;
            self.cap_pnl(true, &pnl, &long_value, pnl_factor)?
        };
        let short_pnl = {
            let pnl = self.pnl(&prices.index_token_price, false, !maximize)?;
            self.cap_pnl(false, &pnl, &short_value, pnl_factor)?
        };
        let net_pnl = long_pnl
            .checked_add(&short_pnl)
            .ok_or(crate::Error::Computation("calculating net pnl"))?;
        pool_value = pool_value
            .checked_sub(&net_pnl)
            .ok_or(crate::Error::Computation("deducting net pnl"))?;

        // Deduct impact pool value.
        let impact_pool_value = {
            let duration = self.passed_in_seconds_for_position_impact_distribution()?;
            let amount = self
                .pending_position_impact_pool_distribution_amount(duration)?
                .1;
            let price = prices.index_token_price.pick_price(!maximize);
            amount
                .checked_mul(price)
                .ok_or(crate::Error::Computation("calculating impact pool value"))?
        }
        .to_signed()?;

        pool_value = pool_value
            .checked_sub(&impact_pool_value)
            .ok_or(crate::Error::Computation("deducting impact pool value"))?;

        Ok(pool_value)
    }

    /// Get market token price.
    fn market_token_price(
        &self,
        prices: &Prices<Self::Num>,
        pnl_factor: PnlFactorKind,
        maximize: bool,
    ) -> crate::Result<Self::Num> {
        let supply = self.total_supply();
        if supply.is_zero() {
            return Ok(Self::Num::UNIT);
        }
        let pool_value = self.pool_value(prices, pnl_factor, maximize)?;
        if pool_value.is_negative() {
            return Err(crate::Error::InvalidPoolValue("the pool value is negative. Calculation of the market token price is currently unsupported when the pool value is negative."));
        }
        let one = Self::Num::UNIT
            .checked_div(&self.usd_to_amount_divisor())
            .ok_or(crate::Error::Computation("calculating one market token"))?;
        crate::utils::market_token_amount_to_usd(&one, &pool_value.unsigned_abs(), &supply)
            .ok_or(crate::Error::Computation("calculating market token price"))
    }
}

impl<M: LiquidityMarket<DECIMALS>, const DECIMALS: u8> LiquidityMarketExt<DECIMALS> for M {}

/// Extension trait of [`LiquidityMarketMut`].
pub trait LiquidityMarketMutExt<const DECIMALS: u8>: LiquidityMarketMut<DECIMALS> {
    /// Create a [`Deposit`] action.
    fn deposit(
        &mut self,
        long_token_amount: Self::Num,
        short_token_amount: Self::Num,
        prices: Prices<Self::Num>,
    ) -> Result<Deposit<&mut Self, DECIMALS>, crate::Error>
    where
        Self: Sized,
    {
        Deposit::try_new(self, long_token_amount, short_token_amount, prices)
    }

    /// Create a [`Withdrawal`].
    fn withdraw(
        &mut self,
        market_token_amount: Self::Num,
        prices: Prices<Self::Num>,
    ) -> crate::Result<Withdrawal<&mut Self, DECIMALS>>
    where
        Self: Sized,
    {
        Withdrawal::try_new(self, market_token_amount, prices)
    }
}

impl<M: LiquidityMarketMut<DECIMALS>, const DECIMALS: u8> LiquidityMarketMutExt<DECIMALS> for M {}
