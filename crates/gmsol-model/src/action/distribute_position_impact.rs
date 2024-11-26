use num_traits::Zero;

use crate::{
    market::{
        BaseMarket, PositionImpactMarketExt, PositionImpactMarketMut, PositionImpactMarketMutExt,
    },
    num::Unsigned,
};

/// Distribute Position Impact.
#[must_use]
pub struct DistributePositionImpact<M: BaseMarket<DECIMALS>, const DECIMALS: u8> {
    market: M,
}

/// Distribute Position Impact Report.
#[derive(Debug)]
pub struct DistributePositionImpactReport<T> {
    duration_in_seconds: u64,
    distribution_amount: T,
    next_position_impact_pool_amount: T,
}

impl<T> DistributePositionImpactReport<T> {
    /// Get considered duration in seconds.
    pub fn duration_in_seconds(&self) -> u64 {
        self.duration_in_seconds
    }

    /// Get distribution amount.
    pub fn distribution_amount(&self) -> &T {
        &self.distribution_amount
    }

    /// Get Next position impact pool amount.
    pub fn next_position_impact_pool_amount(&self) -> &T {
        &self.next_position_impact_pool_amount
    }
}

impl<M: PositionImpactMarketMut<DECIMALS>, const DECIMALS: u8>
    DistributePositionImpact<M, DECIMALS>
{
    /// Execute.
    pub fn execute(mut self) -> crate::Result<DistributePositionImpactReport<M::Num>> {
        let duration_in_seconds = self
            .market
            .just_passed_in_seconds_for_position_impact_distribution()?;

        let (distribution_amount, next_position_impact_pool_amount) = self
            .market
            .pending_position_impact_pool_distribution_amount(duration_in_seconds)?;

        if !distribution_amount.is_zero() {
            self.market
                .apply_delta_to_position_impact_pool(&distribution_amount.to_opposite_signed()?)?;
        }

        Ok(DistributePositionImpactReport {
            duration_in_seconds,
            distribution_amount,
            next_position_impact_pool_amount,
        })
    }
}

impl<M: BaseMarket<DECIMALS>, const DECIMALS: u8> From<M>
    for DistributePositionImpact<M, DECIMALS>
{
    fn from(market: M) -> Self {
        Self { market }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use crate::{
        market::LiquidityMarketMutExt,
        price::Prices,
        test::{TestMarket, TestPosition},
        PositionMutExt,
    };

    use super::*;

    #[test]
    fn test_distribute_position_impact() -> crate::Result<()> {
        let mut market = TestMarket::<u64, 9>::default();
        market.distribute_position_impact()?.execute()?;
        let prices = Prices::new_for_test(120, 120, 1);
        market
            .deposit(1_000_000_000_000, 100_000_000_000_000, prices)?
            .execute()?;
        println!("{market:#?}");
        let mut position = TestPosition::long(true);
        for _ in 0..100 {
            market.distribute_position_impact()?.execute()?;
            _ = position
                .ops(&mut market)
                .increase(
                    Prices::new_for_test(123, 123, 1),
                    1_000_000_000_000,
                    50_000_000_000_000,
                    None,
                )?
                .execute()?;
            market.distribute_position_impact()?.execute()?;
            _ = position
                .ops(&mut market)
                .decrease(
                    Prices::new_for_test(123, 123, 1),
                    50_000_000_000_000,
                    None,
                    0,
                    false,
                    false,
                )?
                .execute()?;
        }
        println!("{market:#?}");
        sleep(Duration::from_secs(1));
        let report = market.distribute_position_impact()?.execute()?;
        println!("{report:#?}");
        println!("{market:#?}");
        Ok(())
    }
}
