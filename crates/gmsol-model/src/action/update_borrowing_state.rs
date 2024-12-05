use crate::{
    market::{BaseMarket, BorrowingFeeMarketExt},
    num::Unsigned,
    price::Prices,
    PerpMarketMut, PoolExt,
};

use super::MarketAction;

/// Update Borrowing State Action.
#[must_use = "actions do nothing unless you `execute` them"]
pub struct UpdateBorrowingState<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
    prices: Prices<M::Num>,
}

impl<M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> UpdateBorrowingState<M, DECIMALS> {
    /// Create a new [`UpdateBorrowingState`] action.
    pub fn try_new(market: M, prices: &Prices<M::Num>) -> crate::Result<Self> {
        prices.validate()?;
        Ok(Self {
            market,
            prices: prices.clone(),
        })
    }

    fn execute_one_side(
        &mut self,
        is_long: bool,
        duration_in_seconds: u64,
    ) -> crate::Result<M::Num> {
        let (next_cumulative_borrowing_factor, delta) = self
            .market
            .next_cumulative_borrowing_factor(is_long, &self.prices, duration_in_seconds)?;
        self.market
            .borrowing_factor_pool_mut()?
            .apply_delta_amount(is_long, &delta.to_signed()?)?;
        Ok(next_cumulative_borrowing_factor)
    }
}

impl<M: PerpMarketMut<DECIMALS>, const DECIMALS: u8> MarketAction
    for UpdateBorrowingState<M, DECIMALS>
{
    type Report = UpdateBorrowingReport<M::Num>;

    fn execute(mut self) -> crate::Result<Self::Report> {
        let duration_in_seconds = self.market.just_passed_in_seconds_for_borrowing()?;
        let next_cumulative_borrowing_factor_for_long =
            self.execute_one_side(true, duration_in_seconds)?;
        let next_cumulative_borrowing_factor_for_short =
            self.execute_one_side(false, duration_in_seconds)?;
        Ok(UpdateBorrowingReport {
            duration_in_seconds,
            next_cumulative_borrowing_factor_for_long,
            next_cumulative_borrowing_factor_for_short,
        })
    }
}

/// Update Borrowing Report.
#[derive(Debug)]
pub struct UpdateBorrowingReport<T> {
    duration_in_seconds: u64,
    next_cumulative_borrowing_factor_for_long: T,
    next_cumulative_borrowing_factor_for_short: T,
}

impl<T> UpdateBorrowingReport<T> {
    /// Get considered duration in seconds.
    pub fn duration_in_seconds(&self) -> u64 {
        self.duration_in_seconds
    }

    /// Get next cumulative borrowing factor
    pub fn next_cumulative_borrowing_factor(&self, is_long: bool) -> &T {
        if is_long {
            &self.next_cumulative_borrowing_factor_for_long
        } else {
            &self.next_cumulative_borrowing_factor_for_short
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use crate::{
        market::LiquidityMarketMutExt,
        test::{TestMarket, TestPosition},
        MarketAction, PositionMutExt,
    };

    use super::*;

    #[test]
    fn test_update_borrowing_state() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        let prices = Prices::new_for_test(120, 120, 1);
        market
            .deposit(1_000_000_000_000, 100_000_000_000_000, prices)?
            .execute()?;
        println!("{market:#?}");
        let mut position = TestPosition::long(true);
        let prices = Prices::new_for_test(123, 123, 1);
        let report = position
            .ops(&mut market)
            .increase(prices, 1_000_000_000_000, 50_000_000_000_000, None)?
            .execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        sleep(Duration::from_secs(2));
        let report = position
            .ops(&mut market)
            .decrease(prices, 50_000_000_000_000, None, 0, Default::default())?
            .execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        Ok(())
    }
}
