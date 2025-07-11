use crate::{
    market::{BaseMarket, BaseMarketExt, BaseMarketMutExt, LiquidityMarketExt, LiquidityMarketMut},
    num::{MulDiv, Unsigned, UnsignedAbs},
    params::Fees,
    pool::delta::BalanceChange,
    price::{Price, Prices},
    utils, BalanceExt, PnlFactorKind, PoolExt,
};
use num_traits::{CheckedAdd, CheckedDiv, Signed, Zero};

use super::MarketAction;

/// A withdrawal.
#[must_use = "actions do nothing unless you `execute` them"]
pub struct Withdrawal<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
    params: WithdrawParams<M::Num>,
}

/// Withdraw params.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
pub struct WithdrawParams<T> {
    market_token_amount: T,
    prices: Prices<T>,
}

#[cfg(feature = "gmsol-utils")]
impl<T: gmsol_utils::InitSpace> gmsol_utils::InitSpace for WithdrawParams<T> {
    const INIT_SPACE: usize = T::INIT_SPACE + Prices::<T>::INIT_SPACE;
}

impl<T> WithdrawParams<T> {
    /// Get market token amount to burn.
    pub fn market_token_amount(&self) -> &T {
        &self.market_token_amount
    }

    /// Get long token price.
    pub fn long_token_price(&self) -> &Price<T> {
        &self.prices.long_token_price
    }

    /// Get short token price.
    pub fn short_token_price(&self) -> &Price<T> {
        &self.prices.short_token_price
    }
}

/// Report of the execution of withdrawal.
#[must_use = "`long_token_output` and `short_token_output` must be used"]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
pub struct WithdrawReport<T> {
    params: WithdrawParams<T>,
    long_token_fees: Fees<T>,
    short_token_fees: Fees<T>,
    long_token_output: T,
    short_token_output: T,
}

#[cfg(feature = "gmsol-utils")]
impl<T: gmsol_utils::InitSpace> gmsol_utils::InitSpace for WithdrawReport<T> {
    const INIT_SPACE: usize =
        WithdrawParams::<T>::INIT_SPACE + 2 * Fees::<T>::INIT_SPACE + 2 * T::INIT_SPACE;
}

impl<T> WithdrawReport<T> {
    /// Get withdraw params.
    pub fn params(&self) -> &WithdrawParams<T> {
        &self.params
    }

    /// Get long token fees.
    pub fn long_token_fees(&self) -> &Fees<T> {
        &self.long_token_fees
    }

    /// Get short token fees.
    pub fn short_token_fees(&self) -> &Fees<T> {
        &self.short_token_fees
    }

    /// Get the output amount of long tokens.
    #[must_use = "the returned amount of long tokens should be transferred out from the market vault"]
    pub fn long_token_output(&self) -> &T {
        &self.long_token_output
    }

    /// Get the output amount of short tokens.
    #[must_use = "the returned amount of short tokens should be transferred out from the market vault"]
    pub fn short_token_output(&self) -> &T {
        &self.short_token_output
    }
}

impl<const DECIMALS: u8, M: LiquidityMarketMut<DECIMALS>> Withdrawal<M, DECIMALS> {
    /// Create a new withdrawal from the given market.
    pub fn try_new(
        market: M,
        market_token_amount: M::Num,
        prices: Prices<M::Num>,
    ) -> crate::Result<Self> {
        if market_token_amount.is_zero() {
            return Err(crate::Error::EmptyWithdrawal);
        }
        prices.validate()?;
        Ok(Self {
            market,
            params: WithdrawParams {
                market_token_amount,
                prices,
            },
        })
    }

    fn output_amounts(&self) -> crate::Result<(M::Num, M::Num)> {
        let pool_value = self.market.pool_value(
            &self.params.prices,
            PnlFactorKind::MaxAfterWithdrawal,
            false,
        )?;
        if pool_value.is_negative() {
            return Err(crate::Error::InvalidPoolValue(
                "withdrawal: current pool value is negative",
            ));
        }
        if pool_value.is_zero() {
            return Err(crate::Error::InvalidPoolValue(
                "withdrawal: current pool value is zero",
            ));
        }
        let total_supply = self.market.total_supply();

        // We use the liquidity pool value instead of the pool value with pending values to calculate the fraction of
        // long token and short token.
        let pool = self.market.liquidity_pool()?;
        let long_token_value =
            pool.long_usd_value(self.params.long_token_price().pick_price(true))?;
        let short_token_value =
            pool.short_usd_value(self.params.short_token_price().pick_price(true))?;
        let total_pool_token_value =
            long_token_value
                .checked_add(&short_token_value)
                .ok_or(crate::Error::Computation(
                    "calculating total liquidity pool value",
                ))?;

        let market_token_value = utils::market_token_amount_to_usd(
            &self.params.market_token_amount,
            &pool_value.unsigned_abs(),
            &total_supply,
        )
        .ok_or(crate::Error::Computation("amount to usd"))?;

        debug_assert!(!self.params.long_token_price().has_zero());
        debug_assert!(!self.params.short_token_price().has_zero());
        let long_token_amount = market_token_value
            .checked_mul_div(&long_token_value, &total_pool_token_value)
            .and_then(|a| a.checked_div(self.params.long_token_price().pick_price(true)))
            .ok_or(crate::Error::Computation("long token amount"))?;
        let short_token_amount = market_token_value
            .checked_mul_div(&short_token_value, &total_pool_token_value)
            .and_then(|a| a.checked_div(self.params.short_token_price().pick_price(true)))
            .ok_or(crate::Error::Computation("short token amount"))?;
        Ok((long_token_amount, short_token_amount))
    }

    fn charge_fees(&self, amount: &mut M::Num) -> crate::Result<Fees<M::Num>> {
        let (amount_after_fees, fees) = self
            .market
            .swap_fee_params()?
            .apply_fees(BalanceChange::Worsened, amount)
            .ok_or(crate::Error::Computation("apply fees"))?;
        *amount = amount_after_fees;
        Ok(fees)
    }
}

impl<const DECIMALS: u8, M: LiquidityMarketMut<DECIMALS>> MarketAction for Withdrawal<M, DECIMALS> {
    type Report = WithdrawReport<M::Num>;

    fn execute(mut self) -> crate::Result<Self::Report> {
        let (mut long_token_amount, mut short_token_amount) = self.output_amounts()?;
        let long_token_fees = self.charge_fees(&mut long_token_amount)?;
        let short_token_fees = self.charge_fees(&mut short_token_amount)?;
        // Apply claimable fees delta.
        let pool = self.market.claimable_fee_pool_mut()?;
        pool.apply_delta_amount(
            true,
            &long_token_fees
                .fee_amount_for_receiver()
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?,
        )?;
        pool.apply_delta_amount(
            false,
            &short_token_fees
                .fee_amount_for_receiver()
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?,
        )?;
        // Apply pool delta.
        // The delta must be the amount leaves the pool: -(amount_after_fees + fee_receiver_amount)

        let delta = long_token_fees
            .fee_amount_for_receiver()
            .checked_add(&long_token_amount)
            .ok_or(crate::Error::Overflow)?
            .to_opposite_signed()?;
        self.market.apply_delta(true, &delta)?;

        let delta = short_token_fees
            .fee_amount_for_receiver()
            .checked_add(&short_token_amount)
            .ok_or(crate::Error::Overflow)?
            .to_opposite_signed()?;
        self.market.apply_delta(false, &delta)?;

        self.market.validate_reserve(&self.params.prices, true)?;
        self.market.validate_reserve(&self.params.prices, false)?;
        self.market.validate_max_pnl(
            &self.params.prices,
            PnlFactorKind::MaxAfterWithdrawal,
            PnlFactorKind::MaxAfterWithdrawal,
        )?;

        self.market.burn(&self.params.market_token_amount)?;

        Ok(WithdrawReport {
            params: self.params,
            long_token_fees,
            short_token_fees,
            long_token_output: long_token_amount,
            short_token_output: short_token_amount,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        market::LiquidityMarketMutExt, pool::Balance, price::Prices, test::TestMarket, BaseMarket,
        LiquidityMarket, MarketAction,
    };

    #[test]
    fn basic() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");
        let before_supply = market.total_supply();
        let before_long_amount = market.liquidity_pool()?.long_amount()?;
        let before_short_amount = market.liquidity_pool()?.short_amount()?;
        let prices = Prices::new_for_test(120, 120, 1);
        let report = market.withdraw(1_000_000_000, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        assert_eq!(
            market.total_supply() + report.params.market_token_amount,
            before_supply
        );
        assert_eq!(
            market.liquidity_pool()?.long_amount()?
                + report.long_token_fees.fee_amount_for_receiver()
                + report.long_token_output,
            before_long_amount
        );
        assert_eq!(
            market.liquidity_pool()?.short_amount()?
                + report.short_token_fees.fee_amount_for_receiver()
                + report.short_token_output,
            before_short_amount
        );
        Ok(())
    }

    /// A test for zero amount withdrawal.
    #[test]
    fn zero_amount_withdrawal() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        let result = market.withdraw(0, prices);
        assert!(result.is_err());
        Ok(())
    }

    /// A test for over amount withdrawal.
    #[test]
    fn over_amount_withdrawal() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000, prices)?.execute()?;
        println!("{market:#?}");

        let result = market.withdraw(1_000_000_000, prices)?.execute();
        assert!(result.is_err());
        println!("{market:#?}");
        Ok(())
    }

    /// A test for small amount withdrawal.
    #[test]
    fn small_amount_withdrawal() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");
        let before_supply = market.total_supply();
        let before_long_amount = market.liquidity_pool()?.long_amount()?;
        let before_short_amount = market.liquidity_pool()?.short_amount()?;
        let prices = Prices::new_for_test(120, 120, 1);

        let small_amount = 1;
        let report = market.withdraw(small_amount, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        assert_eq!(
            market.total_supply() + report.params.market_token_amount,
            before_supply
        );
        assert_eq!(
            market.liquidity_pool()?.long_amount()?
                + report.long_token_fees.fee_amount_for_receiver()
                + report.long_token_output,
            before_long_amount
        );
        assert_eq!(
            market.liquidity_pool()?.short_amount()?
                + report.short_token_fees.fee_amount_for_receiver()
                + report.short_token_output,
            before_short_amount
        );

        Ok(())
    }
}
