use std::fmt;

use crate::{
    market::{BaseMarket, BaseMarketExt},
    num::{MulDiv, Unsigned, UnsignedAbs},
    params::Fees,
    price::{Price, Prices},
    BalanceExt, Delta, PnlFactorKind, Pool, SwapMarketExt, SwapMarketMut,
};

use num_traits::{CheckedAdd, CheckedMul, CheckedNeg, CheckedSub, Signed, Zero};

use super::MarketAction;

/// A swap.
#[must_use = "actions do nothing unless you `execute` them"]
pub struct Swap<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
    params: SwapParams<M::Num>,
}

impl<const DECIMALS: u8, M: SwapMarketMut<DECIMALS>> Swap<M, DECIMALS> {
    /// Create a new swap in the given market.
    pub fn try_new(
        market: M,
        is_token_in_long: bool,
        token_in_amount: M::Num,
        prices: Prices<M::Num>,
    ) -> crate::Result<Self> {
        if token_in_amount.is_zero() {
            return Err(crate::Error::EmptySwap);
        }
        prices.validate()?;
        Ok(Self {
            market,
            params: SwapParams {
                is_token_in_long,
                token_in_amount,
                prices,
            },
        })
    }

    /// Assign the amounts of `token_in` and `token_out` to `long_token` and `short_token`, respectively,
    /// and assign the prices of `long_token` and `short_token` to `token_in` and `token_out`.
    fn reassign_values(&self) -> crate::Result<ReassignedValues<M::Num>> {
        if self.params.is_token_in_long {
            let long_delta_value: M::Signed = self
                .params
                .token_in_amount
                .checked_mul(&self.params.long_token_price().mid())
                .ok_or(crate::Error::Computation("long delta value"))?
                .try_into()
                .map_err(|_| crate::Error::Convert)?;
            Ok(ReassignedValues::new(
                long_delta_value.clone(),
                long_delta_value
                    .checked_neg()
                    .ok_or(crate::Error::Computation("negating long delta value"))?,
                self.params.long_token_price().clone(),
                self.params.short_token_price().clone(),
                PnlFactorKind::MaxAfterDeposit,
                PnlFactorKind::MaxAfterWithdrawal,
            ))
        } else {
            let short_delta_value: M::Signed = self
                .params
                .token_in_amount
                .checked_mul(&self.params.short_token_price().mid())
                .ok_or(crate::Error::Computation("short delta value"))?
                .try_into()
                .map_err(|_| crate::Error::Convert)?;
            Ok(ReassignedValues::new(
                short_delta_value
                    .checked_neg()
                    .ok_or(crate::Error::Computation("negating short delta value"))?,
                short_delta_value,
                self.params.short_token_price().clone(),
                self.params.long_token_price().clone(),
                PnlFactorKind::MaxAfterWithdrawal,
                PnlFactorKind::MaxAfterDeposit,
            ))
        }
    }

    fn charge_fees(&self, is_positive_impact: bool) -> crate::Result<(M::Num, Fees<M::Num>)> {
        self.market
            .swap_fee_params()?
            .apply_fees(is_positive_impact, &self.params.token_in_amount)
            .ok_or(crate::Error::Computation("apply fees"))
    }

    fn try_execute(&self) -> crate::Result<(Cache<'_, M, DECIMALS>, SwapResult<M::Num>)> {
        let ReassignedValues {
            long_token_delta_value,
            short_token_delta_value,
            token_in_price,
            token_out_price,
            long_pnl_factor_kind,
            short_pnl_factor_kind,
        } = self.reassign_values()?;

        // Calculate price impact.
        //
        // Note that the virtual inventory feature is not currently supported.
        let price_impact = self
            .market
            .liquidity_pool()?
            .pool_delta_with_values(
                long_token_delta_value,
                short_token_delta_value,
                &self.params.long_token_price().mid(),
                &self.params.short_token_price().mid(),
            )?
            .price_impact(&self.market.swap_impact_params()?)?;

        let (amount_after_fees, fees) = self.charge_fees(price_impact.is_positive())?;

        let claimable_fee =
            self.market
                .claimable_fee_pool()?
                .checked_apply_delta(Delta::new_one_side(
                    self.params.is_token_in_long,
                    &fees.fee_receiver_amount().to_signed()?,
                ))?;

        // Calculate final amounts && apply delta to price impact pool.
        let token_in_amount;
        let token_out_amount;
        let pool_amount_out;
        let price_impact_amount;
        let swap_impact;
        if price_impact.is_positive() {
            let swap_impact_deduct_side = !self.params.is_token_in_long;
            let signed_price_impact_amount = self.market.swap_impact_amount_with_cap(
                swap_impact_deduct_side,
                &token_out_price,
                &price_impact,
            )?;
            debug_assert!(!signed_price_impact_amount.is_negative());
            swap_impact =
                self.market
                    .swap_impact_pool()?
                    .checked_apply_delta(Delta::new_one_side(
                        swap_impact_deduct_side,
                        &signed_price_impact_amount.checked_neg().ok_or(
                            crate::Error::Computation("negating positive price impact amount "),
                        )?,
                    ))?;
            price_impact_amount = signed_price_impact_amount.unsigned_abs();

            token_in_amount = amount_after_fees;
            pool_amount_out = token_in_amount
                .checked_mul_div(
                    token_in_price.pick_price(false),
                    token_out_price.pick_price(true),
                )
                .ok_or(crate::Error::Computation(
                    "pool amount out for positive impact",
                ))?;
            // Extra amount is deducted from the swap impact pool.
            token_out_amount = pool_amount_out.checked_add(&price_impact_amount).ok_or(
                crate::Error::Computation("token out amount for positive impact"),
            )?;
        } else {
            let swap_impact_deduct_side = self.params.is_token_in_long;
            let signed_price_impact_amount = self.market.swap_impact_amount_with_cap(
                swap_impact_deduct_side,
                &token_in_price,
                &price_impact,
            )?;
            debug_assert!(!signed_price_impact_amount.is_positive());
            swap_impact =
                self.market
                    .swap_impact_pool()?
                    .checked_apply_delta(Delta::new_one_side(
                        swap_impact_deduct_side,
                        &signed_price_impact_amount.checked_neg().ok_or(
                            crate::Error::Computation("negating negative price impact amount "),
                        )?,
                    ))?;
            price_impact_amount = signed_price_impact_amount.unsigned_abs();

            token_in_amount = amount_after_fees.checked_sub(&price_impact_amount).ok_or(
                crate::Error::Computation("swap: not enough fund to pay price impact"),
            )?;

            if token_in_amount.is_zero() {
                return Err(crate::Error::Computation(
                    "swap: not enough fund to pay price impact",
                ));
            }

            token_out_amount = token_in_amount
                .checked_mul_div(
                    token_in_price.pick_price(false),
                    token_out_price.pick_price(true),
                )
                .ok_or(crate::Error::Computation(
                    "token out amount for negative impact",
                ))?;
            pool_amount_out = token_out_amount.clone();
        }

        // Apply delta to liquidity pools.
        // `token_in_amount` is assumed to have been transferred in.
        let liquidity =
            self.market
                .liquidity_pool()?
                .checked_apply_delta(Delta::new_both_sides(
                    self.params.is_token_in_long,
                    &token_in_amount
                        .checked_add(fees.fee_amount_for_pool())
                        .ok_or(crate::Error::Overflow)?
                        .to_signed()?,
                    &pool_amount_out.to_opposite_signed()?,
                ))?;

        let cache = Cache {
            market: &self.market,
            liquidity,
            swap_impact,
            claimable_fee,
        };

        cache.validate_pool_amount(self.params.is_token_in_long)?;
        cache.validate_reserve(&self.params.prices, !self.params.is_token_in_long)?;
        cache.validate_max_pnl(
            &self.params.prices,
            long_pnl_factor_kind,
            short_pnl_factor_kind,
        )?;

        let result = SwapResult {
            price_impact_value: price_impact,
            token_in_fees: fees,
            token_out_amount,
            price_impact_amount,
        };

        Ok((cache, result))
    }
}

impl<const DECIMALS: u8, M> MarketAction for Swap<M, DECIMALS>
where
    M: SwapMarketMut<DECIMALS>,
{
    type Report = SwapReport<M::Num>;

    /// Execute the swap.
    /// # Notes
    /// - This function is atomic.
    fn execute(mut self) -> crate::Result<Self::Report> {
        let (cache, result) = self.try_execute()?;

        let Cache {
            liquidity,
            swap_impact,
            claimable_fee,
            ..
        } = cache;

        *self
            .market
            .liquidity_pool_mut()
            .expect("liquidity pool must be valid") = liquidity;

        *self
            .market
            .swap_impact_pool_mut()
            .expect("swap impact pool must be valid") = swap_impact;

        *self
            .market
            .claimable_fee_pool_mut()
            .expect("claimable fee pool must be valid") = claimable_fee;

        Ok(SwapReport {
            params: self.params,
            result,
        })
    }
}

/// Swap params.
#[derive(Debug, Clone, Copy)]
pub struct SwapParams<T> {
    is_token_in_long: bool,
    token_in_amount: T,
    prices: Prices<T>,
}

impl<T> SwapParams<T> {
    /// Get long token price.
    pub fn long_token_price(&self) -> &Price<T> {
        &self.prices.long_token_price
    }

    /// Get short token price.
    pub fn short_token_price(&self) -> &Price<T> {
        &self.prices.short_token_price
    }

    /// Whether the in token is long token.
    pub fn is_token_in_long(&self) -> bool {
        self.is_token_in_long
    }

    /// Get the amount of in token.
    pub fn token_in_amount(&self) -> &T {
        &self.token_in_amount
    }
}

#[derive(Debug, Clone, Copy)]
struct SwapResult<T: Unsigned> {
    token_in_fees: Fees<T>,
    token_out_amount: T,
    price_impact_value: T::Signed,
    price_impact_amount: T,
}

/// Report of the execution of swap.
#[must_use = "`token_out_amount` must be used"]
pub struct SwapReport<T: Unsigned> {
    params: SwapParams<T>,
    result: SwapResult<T>,
}

impl<T> fmt::Debug for SwapReport<T>
where
    T: Unsigned,
    T: fmt::Debug,
    T::Signed: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwapReport")
            .field("params", &self.params)
            .field("result", &self.result)
            .finish()
    }
}

impl<T: Unsigned> SwapReport<T> {
    /// Get swap params.
    pub fn params(&self) -> &SwapParams<T> {
        &self.params
    }

    /// Get token in fees.
    pub fn token_in_fees(&self) -> &Fees<T> {
        &self.result.token_in_fees
    }

    /// Get the amount of out token.
    #[must_use = "the returned amount of tokens should be transferred out from the market vault"]
    pub fn token_out_amount(&self) -> &T {
        &self.result.token_out_amount
    }

    /// Get the price impact for the swap.
    pub fn price_impact(&self) -> &T::Signed {
        &self.result.price_impact_value
    }

    /// Get the price impact amount.
    pub fn price_impact_amount(&self) -> &T {
        &self.result.price_impact_amount
    }
}

struct ReassignedValues<T: Unsigned> {
    long_token_delta_value: T::Signed,
    short_token_delta_value: T::Signed,
    token_in_price: Price<T>,
    token_out_price: Price<T>,
    long_pnl_factor_kind: PnlFactorKind,
    short_pnl_factor_kind: PnlFactorKind,
}

impl<T: Unsigned> ReassignedValues<T> {
    fn new(
        long_token_delta_value: T::Signed,
        short_token_delta_value: T::Signed,
        token_in_price: Price<T>,
        token_out_price: Price<T>,
        long_pnl_factor_kind: PnlFactorKind,
        short_pnl_factor_kind: PnlFactorKind,
    ) -> Self {
        Self {
            long_token_delta_value,
            short_token_delta_value,
            token_in_price,
            token_out_price,
            long_pnl_factor_kind,
            short_pnl_factor_kind,
        }
    }
}

struct Cache<'a, M, const DECIMALS: u8>
where
    M: BaseMarket<DECIMALS>,
{
    market: &'a M,
    liquidity: M::Pool,
    swap_impact: M::Pool,
    claimable_fee: M::Pool,
}

impl<'a, M, const DECIMALS: u8> BaseMarket<DECIMALS> for Cache<'a, M, DECIMALS>
where
    M: BaseMarket<DECIMALS>,
{
    type Num = M::Num;

    type Signed = M::Signed;

    type Pool = M::Pool;

    fn liquidity_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.liquidity)
    }

    fn claimable_fee_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.claimable_fee)
    }

    fn swap_impact_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.swap_impact)
    }

    fn open_interest_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        self.market.open_interest_pool(is_long)
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        self.market.open_interest_in_tokens_pool(is_long)
    }

    fn collateral_sum_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        self.market.collateral_sum_pool(is_long)
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        self.market.usd_to_amount_divisor()
    }

    fn max_pool_amount(&self, is_long_token: bool) -> crate::Result<Self::Num> {
        self.market.max_pool_amount(is_long_token)
    }

    fn pnl_factor_config(&self, kind: PnlFactorKind, is_long: bool) -> crate::Result<Self::Num> {
        self.market.pnl_factor_config(kind, is_long)
    }

    fn reserve_factor(&self) -> crate::Result<Self::Num> {
        self.market.reserve_factor()
    }

    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num> {
        self.market.open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> crate::Result<Self::Num> {
        self.market.max_open_interest(is_long)
    }

    fn ignore_open_interest_for_usage_factor(&self) -> crate::Result<bool> {
        self.market.ignore_open_interest_for_usage_factor()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        market::{LiquidityMarketMutExt, SwapMarketMutExt},
        pool::Balance,
        price::Prices,
        test::TestMarket,
        BaseMarket, LiquidityMarket, MarketAction,
    };

    #[test]
    fn basic() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let mut prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        prices.index_token_price.set_price_for_test(121);
        prices.long_token_price.set_price_for_test(121);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        prices.index_token_price.set_price_for_test(122);
        prices.long_token_price.set_price_for_test(122);
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");

        let prices = Prices::new_for_test(123, 123, 1);

        // Test for positive impact.
        let before_market = market.clone();
        let token_in_amount = 100_000_000;
        let report = market.swap(false, token_in_amount, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");

        assert_eq!(before_market.total_supply(), market.total_supply());

        assert_eq!(
            before_market.liquidity_pool()?.long_amount()?,
            market.liquidity_pool()?.long_amount()? + report.token_out_amount()
                - report.price_impact_amount(),
        );
        assert_eq!(
            before_market.liquidity_pool()?.short_amount()? + token_in_amount
                - report.token_in_fees().fee_receiver_amount(),
            market.liquidity_pool()?.short_amount()?,
        );

        assert_eq!(
            before_market.swap_impact_pool()?.long_amount()?,
            market.swap_impact_pool()?.long_amount()? + report.price_impact_amount(),
        );
        assert_eq!(
            before_market.swap_impact_pool()?.short_amount()?,
            market.swap_impact_pool()?.short_amount()?
        );

        assert_eq!(
            before_market.claimable_fee_pool()?.long_amount()?,
            market.claimable_fee_pool()?.long_amount()?,
        );
        assert_eq!(
            before_market.claimable_fee_pool()?.short_amount()?
                + report.token_in_fees().fee_receiver_amount(),
            market.claimable_fee_pool()?.short_amount()?,
        );

        // Test for negative impact.
        let before_market = market.clone();
        let token_in_amount = 100_000;

        let prices = Prices::new_for_test(119, 119, 1);

        let report = market.swap(true, token_in_amount, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");

        assert_eq!(before_market.total_supply(), market.total_supply());

        assert_eq!(
            before_market.liquidity_pool()?.long_amount()? + token_in_amount
                - report.price_impact_amount()
                - report.token_in_fees().fee_receiver_amount(),
            market.liquidity_pool()?.long_amount()?,
        );
        assert_eq!(
            before_market.liquidity_pool()?.short_amount()? - report.token_out_amount(),
            market.liquidity_pool()?.short_amount()?,
        );

        assert_eq!(
            before_market.swap_impact_pool()?.long_amount()? + report.price_impact_amount(),
            market.swap_impact_pool()?.long_amount()?,
        );
        assert_eq!(
            before_market.swap_impact_pool()?.short_amount()?,
            market.swap_impact_pool()?.short_amount()?
        );

        assert_eq!(
            before_market.claimable_fee_pool()?.long_amount()?
                + report.token_in_fees().fee_receiver_amount(),
            market.claimable_fee_pool()?.long_amount()?,
        );
        assert_eq!(
            before_market.claimable_fee_pool()?.short_amount()?,
            market.claimable_fee_pool()?.short_amount()?,
        );
        Ok(())
    }

    /// A test for zero swap.
    #[test]
    fn zero_amount_swap() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");

        let result = market.swap(true, 0, prices);
        assert!(result.is_err());
        println!("{market:#?}");

        Ok(())
    }

    /// A test for over amount.
    #[test]
    fn over_amount_swap() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        market.deposit(0, 1_000_000_000, prices)?.execute()?;
        println!("{market:#?}");

        let result = market.swap(true, 2_000_000_000, prices)?.execute();
        assert!(result.is_err());
        println!("{market:#?}");

        // Try to swap out all long token.
        let token_in_amount =
            market.liquidity_pool()?.long_amount()? * prices.long_token_price.mid();
        let report = market.swap(false, token_in_amount, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");

        Ok(())
    }

    /// A test for small amount.
    #[test]
    fn small_amount_swap() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market.deposit(1_000_000_000, 0, prices)?.execute()?;
        println!("{market:#?}");

        let small_amount = 1;

        let report = market.swap(false, small_amount, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        assert!(market.liquidity_pool()?.short_amount()? != 0);

        let report = market
            .swap(false, prices.long_token_price.mid() * small_amount, prices)?
            .execute()?;
        println!("{report:#?}");
        println!("{market:#?}");

        // Test for round.
        let report = market.swap(false, 200, prices)?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");

        Ok(())
    }
}
