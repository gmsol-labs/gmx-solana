use std::{collections::HashMap, fmt, time::Instant};

use crate::{
    clock::ClockKind,
    fixed::FixedPointOps,
    market::{
        BaseMarket, LiquidityMarket, LiquidityMarketMut, PerpMarket, PnlFactorKind,
        PositionImpactMarket, SwapMarket,
    },
    num::{MulDiv, Num, Unsigned, UnsignedAbs},
    params::{
        fee::{BorrowingFeeParams, FundingFeeParams},
        position::PositionImpactDistributionParams,
        FeeParams, PositionParams, PriceImpactParams,
    },
    pool::{Balance, Pool},
    position::Position,
    BaseMarketMut, BorrowingFeeMarket, PerpMarketMut, PositionImpactMarketMut, PositionMut,
    PositionState, PositionStateMut, SwapMarketMut,
};
use num_traits::{CheckedSub, Signed};

/// Test Pool.
#[derive(Debug, Default, Clone, Copy)]
pub struct TestPool<T> {
    long_amount: T,
    short_amount: T,
}

impl<T> Balance for TestPool<T>
where
    T: MulDiv + Num + CheckedSub,
{
    type Num = T;

    type Signed = T::Signed;

    fn long_amount(&self) -> crate::Result<Self::Num> {
        Ok(self.long_amount.clone())
    }

    fn short_amount(&self) -> crate::Result<Self::Num> {
        Ok(self.short_amount.clone())
    }
}

impl<T> Pool for TestPool<T>
where
    T: MulDiv + Num + CheckedSub,
{
    fn apply_delta_to_long_amount(&mut self, delta: &Self::Signed) -> Result<(), crate::Error> {
        if delta.is_positive() {
            self.long_amount = self
                .long_amount
                .checked_add(&delta.unsigned_abs())
                .ok_or(crate::Error::Overflow)?;
        } else {
            self.long_amount = self
                .long_amount
                .checked_sub(&delta.unsigned_abs())
                .ok_or(crate::Error::Computation("decreasing long amount"))?;
        }
        Ok(())
    }

    fn apply_delta_to_short_amount(&mut self, delta: &Self::Signed) -> Result<(), crate::Error> {
        if delta.is_positive() {
            self.short_amount = self
                .short_amount
                .checked_add(&delta.unsigned_abs())
                .ok_or(crate::Error::Overflow)?;
        } else {
            self.short_amount = self
                .short_amount
                .checked_sub(&delta.unsigned_abs())
                .ok_or(crate::Error::Computation("decreasing short amount"))?;
        }
        Ok(())
    }
}

/// Max PnL Factors.
#[derive(Debug, Clone)]
pub struct MaxPnlFactors<T> {
    /// For deposit.
    pub deposit: T,
    /// For withdrawal.
    pub withdrawal: T,
    /// For trader.
    pub trader: T,
    /// For ADL.
    pub adl: T,
}

/// Test Market.
#[derive(Debug, Clone)]
pub struct TestMarket<T: Unsigned, const DECIMALS: u8> {
    config: TestMarketConfig<T, DECIMALS>,
    total_supply: T,
    value_to_amount_divisor: T,
    funding_amount_per_size_adjustment: T,
    primary: TestPool<T>,
    swap_impact: TestPool<T>,
    fee: TestPool<T>,
    open_interest: (TestPool<T>, TestPool<T>),
    open_interest_in_tokens: (TestPool<T>, TestPool<T>),
    position_impact: TestPool<T>,
    borrowing_factor: TestPool<T>,
    funding_factor_per_second: T::Signed,
    funding_amount_per_size: (TestPool<T>, TestPool<T>),
    claimable_funding_amount_per_size: (TestPool<T>, TestPool<T>),
    collateral_sum: (TestPool<T>, TestPool<T>),
    total_borrowing: TestPool<T>,
    clocks: HashMap<ClockKind, Instant>,
}

impl<T: Unsigned, const DECIMALS: u8> TestMarket<T, DECIMALS> {
    /// Create a new test market.
    fn new(
        value_to_amount_divisor: T,
        funding_amount_per_size_adjustment: T,
        config: TestMarketConfig<T, DECIMALS>,
    ) -> Self
    where
        T: Default,
        T::Signed: Default,
    {
        Self {
            config,
            total_supply: Default::default(),
            value_to_amount_divisor,
            funding_amount_per_size_adjustment,
            primary: Default::default(),
            swap_impact: Default::default(),
            fee: Default::default(),
            open_interest: Default::default(),
            open_interest_in_tokens: Default::default(),
            position_impact: Default::default(),
            borrowing_factor: Default::default(),
            funding_factor_per_second: Default::default(),
            funding_amount_per_size: Default::default(),
            claimable_funding_amount_per_size: Default::default(),
            collateral_sum: Default::default(),
            total_borrowing: Default::default(),
            clocks: Default::default(),
        }
    }
}

impl TestMarket<u64, 9> {
    /// Create a new [`TestMarket`] with config.
    pub fn with_config(config: TestMarketConfig<u64, 9>) -> Self {
        Self::new(1, 10_000, config)
    }
}

#[cfg(feature = "u128")]
impl TestMarket<u128, 20> {
    /// Create a new [`TestMarket`] with config.
    pub fn with_config(config: TestMarketConfig<u128, 20>) -> Self {
        Self::new(10u128.pow(20 - 9), 10u128.pow(10), config)
    }
}

impl Default for TestMarket<u64, 9> {
    fn default() -> Self {
        Self::with_config(Default::default())
    }
}

#[cfg(feature = "u128")]
impl Default for TestMarket<u128, 20> {
    fn default() -> Self {
        Self::with_config(Default::default())
    }
}

/// Test Market Config.
#[derive(Debug, Clone)]
pub struct TestMarketConfig<T, const DECIMALS: u8> {
    /// Swap impact params.
    pub swap_impact_params: PriceImpactParams<T>,
    /// Swap fee params.
    pub swap_fee_params: FeeParams<T>,
    /// Position params.
    pub position_params: PositionParams<T>,
    /// Position impact params.
    pub position_impact_params: PriceImpactParams<T>,
    /// Order fee params.
    pub order_fee_params: FeeParams<T>,
    /// Position impact distribution params.
    pub position_impact_distribution_params: PositionImpactDistributionParams<T>,
    /// Borrowing fee params.
    pub borrowing_fee_params: BorrowingFeeParams<T>,
    /// Funding fee params.
    pub funding_fee_params: FundingFeeParams<T>,
    /// Reserve factor.
    pub reserve_factor: T,
    /// Open interest reserve factor.
    pub open_interest_reserve_factor: T,
    /// Max PnL factors.
    pub max_pnl_factors: MaxPnlFactors<T>,
    /// Min PnL factors after ADL.
    pub min_pnl_factor_after_adl: T,
    /// Max pool amount.
    pub max_pool_amount: T,
    /// Max pool value for deposit.
    pub max_pool_value_for_deposit: T,
    /// Max open interest.
    pub max_open_interest: T,
    /// Min collateral factor for OI.
    pub min_collateral_factor_for_oi: T,
}

impl Default for TestMarketConfig<u64, 9> {
    fn default() -> Self {
        Self {
            swap_impact_params: PriceImpactParams::builder()
                .exponent(2_000_000_000)
                .positive_factor(4)
                .negative_factor(8)
                .build(),
            swap_fee_params: FeeParams::builder()
                .fee_receiver_factor(370_000_000)
                .positive_impact_fee_factor(500_000)
                .negative_impact_fee_factor(700_000)
                .build(),
            position_params: PositionParams::new(
                1_000_000_000,
                1_000_000_000,
                10_000_000,
                5_000_000,
                5_000_000,
                2_500_000,
            ),
            position_impact_params: PriceImpactParams::builder()
                .exponent(2_000_000_000)
                .positive_factor(1)
                .negative_factor(2)
                .build(),
            order_fee_params: FeeParams::builder()
                .fee_receiver_factor(370_000_000)
                .positive_impact_fee_factor(500_000)
                .negative_impact_fee_factor(700_000)
                .build(),
            position_impact_distribution_params: PositionImpactDistributionParams::builder()
                .distribute_factor(1_000_000_000)
                .min_position_impact_pool_amount(1_000_000_000)
                .build(),
            borrowing_fee_params: BorrowingFeeParams::builder()
                .receiver_factor(370_000_000)
                .factor_for_long(28)
                .factor_for_short(28)
                .exponent_for_long(1_000_000_000)
                .exponent_for_short(1_000_000_000)
                .build(),
            funding_fee_params: FundingFeeParams::builder()
                .exponent(1_000_000_000)
                .funding_factor(20)
                .max_factor_per_second(10)
                .min_factor_per_second(1)
                .increase_factor_per_second(10)
                .decrease_factor_per_second(0)
                .threshold_for_stable_funding(50_000_000)
                .threshold_for_decrease_funding(0)
                .build(),
            reserve_factor: 1_000_000_000,
            max_pnl_factors: MaxPnlFactors {
                deposit: 600_000_000,
                withdrawal: 300_000_000,
                trader: 500_000_000,
                adl: 500_000_000,
            },
            min_pnl_factor_after_adl: 0,
            open_interest_reserve_factor: 1_000_000_000,
            max_pool_amount: 1_000_000_000 * 1_000_000_000,
            max_pool_value_for_deposit: u64::MAX,
            max_open_interest: u64::MAX,
            // min collateral factor of 0.005 when open interest is $83,000,000
            min_collateral_factor_for_oi: 5 * 10u64.pow(6) / 83_000_000,
        }
    }
}

#[cfg(feature = "u128")]
impl Default for TestMarketConfig<u128, 20> {
    fn default() -> Self {
        Self {
            swap_impact_params: PriceImpactParams::builder()
                .exponent(200_000_000_000_000_000_000)
                .positive_factor(400_000_000_000)
                .negative_factor(800_000_000_000)
                .build(),
            swap_fee_params: FeeParams::builder()
                .fee_receiver_factor(37_000_000_000_000_000_000)
                .positive_impact_fee_factor(50_000_000_000_000_000)
                .negative_impact_fee_factor(70_000_000_000_000_000)
                .build(),
            position_params: PositionParams::new(
                100_000_000_000_000_000_000,
                100_000_000_000_000_000_000,
                1_000_000_000_000_000_000,
                500_000_000_000_000_000,
                500_000_000_000_000_000,
                250_000_000_000_000_000,
            ),
            position_impact_params: PriceImpactParams::builder()
                .exponent(200_000_000_000_000_000_000)
                .positive_factor(100_000_000_000)
                .negative_factor(200_000_000_000)
                .build(),
            order_fee_params: FeeParams::builder()
                .fee_receiver_factor(37_000_000_000_000_000_000)
                .positive_impact_fee_factor(50_000_000_000_000_000)
                .negative_impact_fee_factor(70_000_000_000_000_000)
                .build(),
            position_impact_distribution_params: PositionImpactDistributionParams::builder()
                .distribute_factor(100_000_000_000_000_000_000)
                .min_position_impact_pool_amount(1_000_000_000)
                .build(),
            borrowing_fee_params: BorrowingFeeParams::builder()
                .receiver_factor(37_000_000_000_000_000_000)
                .factor_for_long(2_820_000_000_000)
                .factor_for_short(2_820_000_000_000)
                .exponent_for_long(100_000_000_000_000_000_000)
                .exponent_for_short(100_000_000_000_000_000_000)
                .build(),
            funding_fee_params: FundingFeeParams::builder()
                .exponent(100_000_000_000_000_000_000)
                .funding_factor(2_000_000_000_000)
                .max_factor_per_second(1_000_000_000_000)
                .min_factor_per_second(30_000_000_000)
                .increase_factor_per_second(790_000_000)
                .decrease_factor_per_second(0)
                .threshold_for_stable_funding(5_000_000_000_000_000_000)
                .threshold_for_decrease_funding(0)
                .build(),
            reserve_factor: 10u128.pow(20),
            open_interest_reserve_factor: 10u128.pow(20),
            max_pnl_factors: MaxPnlFactors {
                deposit: 60_000_000_000_000_000_000,
                withdrawal: 30_000_000_000_000_000_000,
                trader: 50_000_000_000_000_000_000,
                adl: 50_000_000_000_000_000_000,
            },
            min_pnl_factor_after_adl: 0,
            max_pool_amount: 1_000_000_000 * 10u128.pow(20),
            max_pool_value_for_deposit: 1_000_000_000_000_000 * 10u128.pow(20),
            max_open_interest: 1_000_000_000 * 10u128.pow(20),
            // min collateral factor of 0.005 when open interest is $83,000,000
            min_collateral_factor_for_oi: 5 * 10u128.pow(17) / 83_000_000,
        }
    }
}

impl<T, const DECIMALS: u8> TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn just_passed_in_seconds(&mut self, clock: ClockKind) -> crate::Result<u64> {
        let now = Instant::now();
        let clock = self.clocks.entry(clock).or_insert(now);
        let duration = now.saturating_duration_since(*clock);
        *clock = now;
        Ok(duration.as_secs())
    }

    fn passed_in_seconds(&self, clock: ClockKind) -> crate::Result<u64> {
        let now = Instant::now();
        let clock = self.clocks.get(&clock).unwrap_or(&now);
        let duration = now.saturating_duration_since(*clock);
        Ok(duration.as_secs())
    }
}

impl<T, const DECIMALS: u8> BaseMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    type Num = T;

    type Signed = T::Signed;

    type Pool = TestPool<T>;

    fn liquidity_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.primary)
    }

    fn claimable_fee_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.fee)
    }

    fn swap_impact_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.swap_impact)
    }

    fn open_interest_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        if is_long {
            Ok(&self.open_interest.0)
        } else {
            Ok(&self.open_interest.1)
        }
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        if is_long {
            Ok(&self.open_interest_in_tokens.0)
        } else {
            Ok(&self.open_interest_in_tokens.1)
        }
    }

    fn collateral_sum_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        if is_long {
            Ok(&self.collateral_sum.0)
        } else {
            Ok(&self.collateral_sum.1)
        }
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        self.value_to_amount_divisor.clone()
    }

    fn max_pool_amount(&self, _is_long_token: bool) -> crate::Result<Self::Num> {
        Ok(self.config.max_pool_amount.clone())
    }

    fn pnl_factor_config(&self, kind: PnlFactorKind, _is_long: bool) -> crate::Result<Self::Num> {
        let factor = match kind {
            PnlFactorKind::MaxAfterDeposit => self.config.max_pnl_factors.deposit.clone(),
            PnlFactorKind::MaxAfterWithdrawal => self.config.max_pnl_factors.withdrawal.clone(),
            PnlFactorKind::MaxForTrader => self.config.max_pnl_factors.trader.clone(),
            PnlFactorKind::ForAdl => self.config.max_pnl_factors.adl.clone(),
            PnlFactorKind::MinAfterAdl => self.config.min_pnl_factor_after_adl.clone(),
        };
        Ok(factor)
    }

    fn reserve_factor(&self) -> crate::Result<Self::Num> {
        Ok(self.config.reserve_factor.clone())
    }
}

impl<T, const DECIMALS: u8> BaseMarketMut<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn liquidity_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.primary)
    }

    fn claimable_fee_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.fee)
    }
}

impl<T, const DECIMALS: u8> SwapMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn swap_impact_params(&self) -> crate::Result<PriceImpactParams<Self::Num>> {
        Ok(self.config.swap_impact_params.clone())
    }

    fn swap_fee_params(&self) -> crate::Result<FeeParams<Self::Num>> {
        Ok(self.config.swap_fee_params.clone())
    }
}

impl<T, const DECIMALS: u8> SwapMarketMut<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn swap_impact_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.swap_impact)
    }
}

impl<T, const DECIMALS: u8> LiquidityMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn total_supply(&self) -> Self::Num {
        self.total_supply.clone()
    }

    fn max_pool_value_for_deposit(&self, _is_long_token: bool) -> crate::Result<Self::Num> {
        Ok(self.config.max_pool_value_for_deposit.clone())
    }
}

impl<T, const DECIMALS: u8> LiquidityMarketMut<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn mint(&mut self, amount: &Self::Num) -> Result<(), crate::Error> {
        self.total_supply = self
            .total_supply
            .checked_add(amount)
            .ok_or(crate::Error::Overflow)?;
        Ok(())
    }

    fn burn(&mut self, amount: &Self::Num) -> crate::Result<()> {
        self.total_supply = self
            .total_supply
            .checked_sub(amount)
            .ok_or(crate::Error::Computation("burning market tokens"))?;
        Ok(())
    }
}

impl<T, const DECIMALS: u8> PositionImpactMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn position_impact_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.position_impact)
    }

    fn position_impact_params(&self) -> crate::Result<PriceImpactParams<Self::Num>> {
        Ok(self.config.position_impact_params.clone())
    }

    fn position_impact_distribution_params(
        &self,
    ) -> crate::Result<PositionImpactDistributionParams<Self::Num>> {
        Ok(self.config.position_impact_distribution_params.clone())
    }

    fn passed_in_seconds_for_position_impact_distribution(&self) -> crate::Result<u64> {
        self.passed_in_seconds(ClockKind::PriceImpactDistribution)
    }
}

impl<T, const DECIMALS: u8> PositionImpactMarketMut<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn position_impact_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.position_impact)
    }

    fn just_passed_in_seconds_for_position_impact_distribution(&mut self) -> crate::Result<u64> {
        self.just_passed_in_seconds(ClockKind::PriceImpactDistribution)
    }
}

impl<T, const DECIMALS: u8> BorrowingFeeMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn borrowing_fee_params(&self) -> crate::Result<BorrowingFeeParams<Self::Num>> {
        Ok(self.config.borrowing_fee_params.clone())
    }

    fn borrowing_factor_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.borrowing_factor)
    }

    fn total_borrowing_pool(&self) -> crate::Result<&Self::Pool> {
        Ok(&self.total_borrowing)
    }

    fn passed_in_seconds_for_borrowing(&self) -> crate::Result<u64> {
        self.passed_in_seconds(ClockKind::Borrowing)
    }
}

impl<T, const DECIMALS: u8> PerpMarket<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn funding_factor_per_second(&self) -> &Self::Signed {
        &self.funding_factor_per_second
    }

    fn funding_amount_per_size_adjustment(&self) -> Self::Num {
        self.funding_amount_per_size_adjustment.clone()
    }

    fn funding_fee_params(&self) -> crate::Result<FundingFeeParams<Self::Num>> {
        Ok(self.config.funding_fee_params.clone())
    }

    fn funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        if is_long {
            Ok(&self.funding_amount_per_size.0)
        } else {
            Ok(&self.funding_amount_per_size.1)
        }
    }

    fn claimable_funding_amount_per_size_pool(&self, is_long: bool) -> crate::Result<&Self::Pool> {
        if is_long {
            Ok(&self.claimable_funding_amount_per_size.0)
        } else {
            Ok(&self.claimable_funding_amount_per_size.1)
        }
    }

    fn position_params(&self) -> crate::Result<PositionParams<Self::Num>> {
        Ok(self.config.position_params.clone())
    }

    fn order_fee_params(&self) -> crate::Result<FeeParams<Self::Num>> {
        Ok(self.config.order_fee_params.clone())
    }

    fn open_interest_reserve_factor(&self) -> crate::Result<Self::Num> {
        Ok(self.config.open_interest_reserve_factor.clone())
    }

    fn max_open_interest(&self, _is_long: bool) -> crate::Result<Self::Num> {
        Ok(self.config.max_open_interest.clone())
    }

    fn min_collateral_factor_for_open_interest_multiplier(
        &self,
        _is_long: bool,
    ) -> crate::Result<Self::Num> {
        Ok(self.config.min_collateral_factor_for_oi.clone())
    }
}

impl<T, const DECIMALS: u8> PerpMarketMut<DECIMALS> for TestMarket<T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn funding_factor_per_second_mut(&mut self) -> &mut Self::Signed {
        &mut self.funding_factor_per_second
    }

    fn open_interest_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool> {
        if is_long {
            Ok(&mut self.open_interest.0)
        } else {
            Ok(&mut self.open_interest.1)
        }
    }

    fn open_interest_in_tokens_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        if is_long {
            Ok(&mut self.open_interest_in_tokens.0)
        } else {
            Ok(&mut self.open_interest_in_tokens.1)
        }
    }

    fn borrowing_factor_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.borrowing_factor)
    }

    fn funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        if is_long {
            Ok(&mut self.funding_amount_per_size.0)
        } else {
            Ok(&mut self.funding_amount_per_size.1)
        }
    }

    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> crate::Result<&mut Self::Pool> {
        if is_long {
            Ok(&mut self.claimable_funding_amount_per_size.0)
        } else {
            Ok(&mut self.claimable_funding_amount_per_size.1)
        }
    }

    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> crate::Result<&mut Self::Pool> {
        if is_long {
            Ok(&mut self.collateral_sum.0)
        } else {
            Ok(&mut self.collateral_sum.1)
        }
    }

    fn total_borrowing_pool_mut(&mut self) -> crate::Result<&mut Self::Pool> {
        Ok(&mut self.total_borrowing)
    }

    fn just_passed_in_seconds_for_borrowing(&mut self) -> crate::Result<u64> {
        self.just_passed_in_seconds(ClockKind::Borrowing)
    }

    fn just_passed_in_seconds_for_funding(&mut self) -> crate::Result<u64> {
        self.just_passed_in_seconds(ClockKind::Funding)
    }
}

/// Test Position
#[derive(Debug, Clone, Copy, Default)]
pub struct TestPosition<T, const DECIMALS: u8> {
    is_long: bool,
    is_collateral_token_long: bool,
    collateral_token_amount: T,
    size_in_usd: T,
    size_in_tokens: T,
    borrowing_factor: T,
    funding_fee_amount_per_size: T,
    claimable_funding_fee_amount_per_size: (T, T),
}

impl<T: Unsigned, const DECIMALS: u8> TestPosition<T, DECIMALS>
where
    T::Signed: fmt::Debug,
{
    /// Create a [`TestPositionOps`] for ops.
    pub fn ops<'a>(
        &'a mut self,
        market: &'a mut TestMarket<T, DECIMALS>,
    ) -> TestPositionOps<T, DECIMALS> {
        TestPositionOps {
            market,
            position: self,
        }
    }

    /// Create an empty long position.
    pub fn long(long_token_as_collateral: bool) -> Self
    where
        T: Default,
    {
        Self {
            is_long: true,
            is_collateral_token_long: long_token_as_collateral,
            ..Default::default()
        }
    }

    /// Create an empty short position.
    pub fn short(long_token_as_collateral: bool) -> Self
    where
        T: Default,
    {
        Self {
            is_long: false,
            is_collateral_token_long: long_token_as_collateral,
            ..Default::default()
        }
    }
}

/// Test Position.
#[derive(Debug)]
pub struct TestPositionOps<'a, T: Unsigned, const DECIMALS: u8>
where
    T::Signed: fmt::Debug,
{
    market: &'a mut TestMarket<T, DECIMALS>,
    position: &'a mut TestPosition<T, DECIMALS>,
}

impl<'a, T, const DECIMALS: u8> PositionState<DECIMALS> for TestPositionOps<'a, T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    type Num = T;

    type Signed = T::Signed;

    fn collateral_amount(&self) -> &Self::Num {
        &self.position.collateral_token_amount
    }

    fn size_in_usd(&self) -> &Self::Num {
        &self.position.size_in_usd
    }

    fn size_in_tokens(&self) -> &Self::Num {
        &self.position.size_in_tokens
    }

    fn borrowing_factor(&self) -> &Self::Num {
        &self.position.borrowing_factor
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        &self.position.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        if is_long_collateral {
            &self.position.claimable_funding_fee_amount_per_size.0
        } else {
            &self.position.claimable_funding_fee_amount_per_size.1
        }
    }
}

impl<'a, T, const DECIMALS: u8> Position<DECIMALS> for TestPositionOps<'a, T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    type Market = TestMarket<T, DECIMALS>;

    fn market(&self) -> &Self::Market {
        self.market
    }

    fn is_long(&self) -> bool {
        self.position.is_long
    }

    fn is_collateral_token_long(&self) -> bool {
        self.position.is_collateral_token_long
    }

    fn are_pnl_and_collateral_tokens_the_same(&self) -> bool {
        self.position.is_long == self.position.is_collateral_token_long
    }
}

impl<'a, T, const DECIMALS: u8> PositionMut<DECIMALS> for TestPositionOps<'a, T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn market_mut(&mut self) -> &mut Self::Market {
        self.market
    }

    fn increased(&mut self) -> crate::Result<()> {
        Ok(())
    }

    fn decreased(&mut self) -> crate::Result<()> {
        Ok(())
    }
}

impl<'a, T, const DECIMALS: u8> PositionStateMut<DECIMALS> for TestPositionOps<'a, T, DECIMALS>
where
    T: CheckedSub + fmt::Display + FixedPointOps<DECIMALS>,
    T::Signed: Num + std::fmt::Debug,
{
    fn collateral_amount_mut(&mut self) -> &mut Self::Num {
        &mut self.position.collateral_token_amount
    }

    fn size_in_usd_mut(&mut self) -> &mut Self::Num {
        &mut self.position.size_in_usd
    }

    fn size_in_tokens_mut(&mut self) -> &mut Self::Num {
        &mut self.position.size_in_tokens
    }

    fn borrowing_factor_mut(&mut self) -> &mut Self::Num {
        &mut self.position.borrowing_factor
    }

    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num {
        &mut self.position.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num {
        if is_long_collateral {
            &mut self.position.claimable_funding_fee_amount_per_size.0
        } else {
            &mut self.position.claimable_funding_fee_amount_per_size.1
        }
    }
}
