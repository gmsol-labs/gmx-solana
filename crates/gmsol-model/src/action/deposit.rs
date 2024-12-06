use num_traits::{CheckedAdd, CheckedMul, CheckedSub, Signed, Zero};

use crate::{
    market::{
        BaseMarket, BaseMarketExt, BaseMarketMutExt, LiquidityMarketExt, LiquidityMarketMut,
        SwapMarketMutExt,
    },
    num::{MulDiv, UnsignedAbs},
    params::Fees,
    price::{Price, Prices},
    utils, BalanceExt, PnlFactorKind, PoolExt,
};

use super::MarketAction;

/// A deposit.
#[must_use = "actions do nothing unless you `execute` them"]
pub struct Deposit<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
    params: DepositParams<M::Num>,
}

/// Deposit params.
#[derive(Debug, Clone, Copy)]
pub struct DepositParams<T> {
    long_token_amount: T,
    short_token_amount: T,
    prices: Prices<T>,
}

impl<T> DepositParams<T> {
    /// Get long token amount.
    pub fn long_token_amount(&self) -> &T {
        &self.long_token_amount
    }

    /// Get short token amount.
    pub fn short_token_amount(&self) -> &T {
        &self.short_token_amount
    }

    /// Get long token price.
    pub fn long_token_price(&self) -> &Price<T> {
        &self.prices.long_token_price
    }

    /// Get short token price.
    pub fn short_token_price(&self) -> &Price<T> {
        &self.prices.short_token_price
    }

    fn reassign_values(&self, is_long_token: bool) -> ReassignedVaules<T>
    where
        T: Clone,
    {
        if is_long_token {
            ReassignedVaules {
                amount: self.long_token_amount.clone(),
                price: self.long_token_price(),
                opposite_price: self.short_token_price(),
            }
        } else {
            ReassignedVaules {
                amount: self.short_token_amount.clone(),
                price: self.short_token_price(),
                opposite_price: self.long_token_price(),
            }
        }
    }
}

/// Report of the execution of deposit.
#[derive(Debug, Clone, Copy)]
pub struct DepositReport<T>
where
    T: MulDiv,
{
    params: DepositParams<T>,
    minted: T,
    price_impact: T::Signed,
    fees: [Fees<T>; 2],
}

impl<T> DepositReport<T>
where
    T: MulDiv,
{
    fn new(
        params: DepositParams<T>,
        price_impact: T::Signed,
        minted: T,
        fees: [Fees<T>; 2],
    ) -> Self {
        Self {
            params,
            minted,
            price_impact,
            fees,
        }
    }

    /// Get minted.
    pub fn minted(&self) -> &T {
        &self.minted
    }

    /// Get price impact.
    pub fn price_impact(&self) -> &T::Signed {
        &self.price_impact
    }

    /// Get the deposit params.
    pub fn params(&self) -> &DepositParams<T> {
        &self.params
    }

    /// Get long token fees.
    pub fn long_token_fees(&self) -> &Fees<T> {
        &self.fees[0]
    }

    /// Get short token fees.
    pub fn short_token_fees(&self) -> &Fees<T> {
        &self.fees[1]
    }
}

impl<const DECIMALS: u8, M: LiquidityMarketMut<DECIMALS>> Deposit<M, DECIMALS> {
    /// Create a new deposit to the given market.
    pub fn try_new(
        market: M,
        long_token_amount: M::Num,
        short_token_amount: M::Num,
        prices: Prices<M::Num>,
    ) -> Result<Self, crate::Error> {
        if long_token_amount.is_zero() && short_token_amount.is_zero() {
            return Err(crate::Error::EmptyDeposit);
        }
        Ok(Self {
            market,
            params: DepositParams {
                long_token_amount,
                short_token_amount,
                prices,
            },
        })
    }

    /// Get the price impact USD value.
    fn price_impact(&self) -> crate::Result<(M::Signed, M::Num, M::Num)> {
        let delta = self.market.liquidity_pool()?.pool_delta_with_amounts(
            &self
                .params
                .long_token_amount
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?,
            &self
                .params
                .short_token_amount
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?,
            &self.params.long_token_price().mid(),
            &self.params.short_token_price().mid(),
        )?;
        let price_impact = delta.price_impact(&self.market.swap_impact_params()?)?;
        let delta = delta.delta();
        debug_assert!(!delta.long_value().is_negative(), "must be non-negative");
        debug_assert!(!delta.short_value().is_negative(), "must be non-negative");
        Ok((
            price_impact,
            delta.long_value().unsigned_abs(),
            delta.short_value().unsigned_abs(),
        ))
    }

    /// Charge swap fees.
    ///
    /// The `amount` will become the amount after fees.
    fn charge_fees(
        &self,
        is_positive_impact: bool,
        amount: &mut M::Num,
    ) -> crate::Result<Fees<M::Num>> {
        let (amount_after_fees, fees) = self
            .market
            .swap_fee_params()?
            .apply_fees(is_positive_impact, amount)
            .ok_or(crate::Error::Computation("apply fees"))?;
        *amount = amount_after_fees;
        Ok(fees)
    }

    fn execute_deposit(
        &mut self,
        is_long_token: bool,
        pool_value: M::Num,
        mut price_impact: M::Signed,
    ) -> Result<(M::Num, Fees<M::Num>), crate::Error> {
        let mut mint_amount: M::Num = Zero::zero();
        let supply = self.market.total_supply();

        if pool_value.is_zero() && !supply.is_zero() {
            return Err(crate::Error::InvalidPoolValue("deposit"));
        }

        let ReassignedVaules {
            mut amount,
            price,
            opposite_price,
        } = self.params.reassign_values(is_long_token);

        let fees = self.charge_fees(price_impact.is_positive(), &mut amount)?;
        self.market.claimable_fee_pool_mut()?.apply_delta_amount(
            is_long_token,
            &fees
                .fee_amount_for_receiver()
                .clone()
                .try_into()
                .map_err(|_| crate::Error::Convert)?,
        )?;

        if price_impact.is_positive() && supply.is_zero() {
            price_impact = Zero::zero();
        }
        if price_impact.is_positive() {
            let positive_impact_amount = self.market.apply_swap_impact_value_with_cap(
                !is_long_token,
                opposite_price,
                &price_impact,
            )?;
            mint_amount = mint_amount
                .checked_add(
                    &utils::usd_to_market_token_amount(
                        positive_impact_amount
                            .checked_mul(opposite_price.pick_price(true))
                            .ok_or(crate::Error::Overflow)?,
                        pool_value.clone(),
                        supply.clone(),
                        self.market.usd_to_amount_divisor(),
                    )
                    .ok_or(crate::Error::Computation("convert positive usd to amount"))?,
                )
                .ok_or(crate::Error::Overflow)?;
            self.market.apply_delta(
                !is_long_token,
                &positive_impact_amount
                    .try_into()
                    .map_err(|_| crate::Error::Convert)?,
            )?;
            self.market.validate_pool_amount(!is_long_token)?;
        } else if price_impact.is_negative() {
            let negative_impact_amount = self.market.apply_swap_impact_value_with_cap(
                is_long_token,
                price,
                &price_impact,
            )?;
            amount =
                amount
                    .checked_sub(&negative_impact_amount)
                    .ok_or(crate::Error::Computation(
                        "deposit: not enough fund to pay negative impact amount",
                    ))?;
        }
        mint_amount = mint_amount
            .checked_add(
                &utils::usd_to_market_token_amount(
                    amount
                        .checked_mul(price.pick_price(false))
                        .ok_or(crate::Error::Overflow)?,
                    pool_value,
                    supply.clone(),
                    self.market.usd_to_amount_divisor(),
                )
                .ok_or(crate::Error::Computation("convert negative usd to amount"))?,
            )
            .ok_or(crate::Error::Overflow)?;
        self.market.apply_delta(
            is_long_token,
            &(amount
                .checked_add(fees.fee_amount_for_pool())
                .ok_or(crate::Error::Overflow)?)
            .clone()
            .try_into()
            .map_err(|_| crate::Error::Convert)?,
        )?;
        self.market.validate_pool_amount(is_long_token)?;
        self.market
            .validate_pool_value_for_deposit(&self.params.prices, is_long_token)?;
        Ok((mint_amount, fees))
    }
}

impl<const DECIMALS: u8, M> MarketAction for Deposit<M, DECIMALS>
where
    M: LiquidityMarketMut<DECIMALS>,
{
    type Report = DepositReport<M::Num>;

    fn execute(mut self) -> crate::Result<Self::Report> {
        debug_assert!(
            !self.params.long_token_amount.is_zero() || !self.params.short_token_amount.is_zero(),
            "shouldn't be empty deposit"
        );

        // Validate max pnl first.
        // Deposits should improve the pool state but it should be checked if
        // the max pnl factor for deposits is exceeded as this would lead to the
        // price of the market token decreasing below a target minimum percentage
        // due to pnl.
        // Note that this is just a validation for deposits, there is no actual
        // minimum price for a market token
        self.market.validate_max_pnl(
            &self.params.prices,
            PnlFactorKind::MaxAfterDeposit,
            PnlFactorKind::MaxAfterDeposit,
        )?;

        let report = {
            let (price_impact, long_token_usd_value, short_token_usd_value) =
                self.price_impact()?;
            let mut market_token_to_mint: M::Num = Zero::zero();
            let pool_value = self.market.pool_value(
                &self.params.prices,
                PnlFactorKind::MaxAfterDeposit,
                true,
            )?;
            if pool_value.is_negative() {
                return Err(crate::Error::InvalidPoolValue(
                    "deposit: current pool value is negative",
                ));
            }
            let mut all_fees = [Default::default(), Default::default()];
            if !self.params.long_token_amount.is_zero() {
                let price_impact = long_token_usd_value
                    .clone()
                    .checked_mul_div_with_signed_numerator(
                        &price_impact,
                        &long_token_usd_value
                            .checked_add(&short_token_usd_value)
                            .ok_or(crate::Error::Overflow)?,
                    )
                    .ok_or(crate::Error::Computation("price impact for long"))?;
                let (mint_amount, fees) =
                    self.execute_deposit(true, pool_value.unsigned_abs(), price_impact)?;
                market_token_to_mint = market_token_to_mint
                    .checked_add(&mint_amount)
                    .ok_or(crate::Error::Overflow)?;
                all_fees[0] = fees;
            }
            if !self.params.short_token_amount.is_zero() {
                let price_impact = short_token_usd_value
                    .clone()
                    .checked_mul_div_with_signed_numerator(
                        &price_impact,
                        &long_token_usd_value
                            .checked_add(&short_token_usd_value)
                            .ok_or(crate::Error::Overflow)?,
                    )
                    .ok_or(crate::Error::Computation("price impact for short"))?;
                let (mint_amount, fees) =
                    self.execute_deposit(false, pool_value.unsigned_abs(), price_impact)?;
                market_token_to_mint = market_token_to_mint
                    .checked_add(&mint_amount)
                    .ok_or(crate::Error::Overflow)?;
                all_fees[1] = fees;
            }
            DepositReport::new(self.params, price_impact, market_token_to_mint, all_fees)
        };
        self.market.mint(&report.minted)?;
        Ok(report)
    }
}

struct ReassignedVaules<'a, T> {
    amount: T,
    price: &'a Price<T>,
    opposite_price: &'a Price<T>,
}

#[cfg(test)]
mod tests {
    use crate::{
        market::LiquidityMarketMutExt,
        price::Prices,
        test::{TestMarket, TestMarketConfig},
        MarketAction,
    };

    #[test]
    fn basic() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u64, 9>::with_config(TestMarketConfig {
            reserve_factor: 1_050_000_000,
            ..Default::default()
        });
        let prices = Prices::new_for_test(120, 120, 1);
        println!(
            "{:#?}",
            market.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!("{market:#?}");
        println!(
            "{:#?}",
            market.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!("{market:#?}");
        println!(
            "{:#?}",
            market.deposit(0, 1_000_000_000, prices)?.execute()?
        );
        println!("{market:#?}");
        Ok(())
    }

    #[test]
    fn sequence() -> crate::Result<()> {
        let mut market_1 = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        println!(
            "{:#?}",
            market_1.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!(
            "{:#?}",
            market_1.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!("{market_1:#?}");
        let mut market_2 = TestMarket::<u64, 9>::default();
        println!(
            "{:#?}",
            market_2.deposit(2_000_000_000, 0, prices)?.execute()?
        );
        println!("{market_1:#?}");
        println!("{market_2:#?}");
        Ok(())
    }

    #[cfg(feature = "u128")]
    #[test]
    fn basic_u128() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u128, 20>::default();
        let prices = Prices::new_for_test(12_000_000_000_000, 12_000_000_000_000, 100_000_000_000);
        println!(
            "{:#?}",
            market.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!(
            "{:#?}",
            market.deposit(1_000_000_000, 0, prices)?.execute()?
        );
        println!(
            "{:#?}",
            market.deposit(0, 1_000_000_000, prices)?.execute()?
        );
        println!("{market:#?}");
        Ok(())
    }

    /// A test for zero amount deposit.
    #[test]
    fn zero_amount_deposit() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        let result = market.deposit(0, 0, prices);
        assert!(result.is_err());

        Ok(())
    }

    /// A test for large and small deposit.
    #[test]
    fn extreme_amount_deposit() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        let small_amount = 1;
        let large_amount = u64::MAX;
        let max_pool_amount = 1000000000000000000;
        println!("{:#?}", market.deposit(small_amount, 0, prices)?.execute()?);
        println!("{market:#?}");

        let result = market.deposit(large_amount, 0, prices)?.execute();
        assert!(result.is_err());
        println!("{market:#?}");

        let result = market.deposit(max_pool_amount, 0, prices)?.execute();
        assert!(result.is_err());
        println!("{market:#?}");

        Ok(())
    }

    /// A test for round attack.
    #[test]
    fn round_attack_deposit() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        let mut i = 1;
        while i < 10000000 {
            i += 1;
            market.deposit(1, 0, prices)?.execute()?;
        }
        println!("{market:#?}");

        let mut market_compare = TestMarket::<u64, 9>::default();
        market_compare.deposit(10000000 - 1, 0, prices)?.execute()?;
        println!("{market_compare:#?}");
        Ok(())
    }

    #[test]
    fn concurrent_deposits() -> Result<(), crate::Error> {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let market = Arc::new(Mutex::new(TestMarket::<u64, 9>::default()));
        let prices = Prices::new_for_test(120, 120, 1);

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let market = Arc::clone(&market);
                thread::spawn(move || {
                    let mut market = market.lock().unwrap();
                    market
                        .deposit(1_000_000_000, 0, prices)
                        .unwrap()
                        .execute()
                        .unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let market = market.lock().unwrap();
        println!("{:#?}", *market);
        Ok(())
    }

    #[test]
    fn deposit_with_price_fluctuations() -> Result<(), crate::Error> {
        let mut market = TestMarket::<u64, 9>::default();
        let initial_prices = Prices::new_for_test(120, 120, 1);
        let fluctuated_prices = Prices::new_for_test(240, 240, 1);
        println!(
            "{:#?}",
            market
                .deposit(1_000_000_000, 0, initial_prices)?
                .execute()?
        );
        println!("{market:#?}");

        println!(
            "{:#?}",
            market
                .deposit(1_000_000_000, 0, fluctuated_prices)?
                .execute()?
        );
        println!("{market:#?}");
        Ok(())
    }
}
