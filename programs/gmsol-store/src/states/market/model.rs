use anchor_spl::token::Mint;
use gmsol_model::{
    params::{
        fee::{BorrowingFeeParams, FundingFeeParams},
        position::PositionImpactDistributionParams,
        FeeParams, PositionParams, PriceImpactParams,
    },
    BorrowingFeeMarket, PoolKind,
};

use crate::constants;

use super::{clock::AsClock, Market, Pool};

impl gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }> for Market {
    type Num = u128;

    type Signed = i128;

    type Pool = Pool;

    fn liquidity_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::Primary)
    }

    fn claimable_fee_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::ClaimableFee)
    }

    fn swap_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::SwapImpact)
    }

    fn open_interest_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(if is_long {
            PoolKind::OpenInterestForLong
        } else {
            PoolKind::OpenInterestForShort
        })
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(if is_long {
            PoolKind::OpenInterestInTokensForLong
        } else {
            PoolKind::OpenInterestInTokensForShort
        })
    }

    fn collateral_sum_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        let kind = if is_long {
            PoolKind::CollateralSumForLong
        } else {
            PoolKind::CollateralSumForShort
        };
        self.try_pool(kind)
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        constants::MARKET_USD_TO_AMOUNT_DIVISOR
    }

    fn max_pool_amount(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        if is_long_token {
            Ok(self.config.max_pool_amount_for_long_token)
        } else {
            Ok(self.config.max_pool_amount_for_short_token)
        }
    }

    fn pnl_factor_config(
        &self,
        kind: gmsol_model::PnlFactorKind,
        is_long: bool,
    ) -> gmsol_model::Result<Self::Num> {
        use gmsol_model::PnlFactorKind;

        match (kind, is_long) {
            (PnlFactorKind::MaxAfterDeposit, true) => {
                Ok(self.config.max_pnl_factor_for_long_deposit)
            }
            (PnlFactorKind::MaxAfterDeposit, false) => {
                Ok(self.config.max_pnl_factor_for_short_deposit)
            }
            (PnlFactorKind::MaxAfterWithdrawal, true) => {
                Ok(self.config.max_pnl_factor_for_long_withdrawal)
            }
            (PnlFactorKind::MaxAfterWithdrawal, false) => {
                Ok(self.config.max_pnl_factor_for_short_withdrawal)
            }
            (PnlFactorKind::MaxForTrader, true) => Ok(self.config.max_pnl_factor_for_long_trader),
            (PnlFactorKind::MaxForTrader, false) => Ok(self.config.max_pnl_factor_for_short_trader),
            (PnlFactorKind::ForAdl, true) => Ok(self.config.max_pnl_factor_for_long_adl),
            (PnlFactorKind::ForAdl, false) => Ok(self.config.max_pnl_factor_for_short_adl),
            (PnlFactorKind::MinAfterAdl, true) => Ok(self.config.min_pnl_factor_after_long_adl),
            (PnlFactorKind::MinAfterAdl, false) => Ok(self.config.min_pnl_factor_after_short_adl),
            _ => Err(gmsol_model::Error::invalid_argument("missing pnl factor")),
        }
    }

    fn reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        Ok(self.config.reserve_factor)
    }
}

impl gmsol_model::SwapMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn swap_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        PriceImpactParams::builder()
            .with_exponent(self.config.swap_impact_exponent)
            .with_positive_factor(self.config.swap_impact_positive_factor)
            .with_negative_factor(self.config.swap_impact_negative_factor)
            .build()
    }

    fn swap_fee_params(&self) -> gmsol_model::Result<FeeParams<Self::Num>> {
        Ok(FeeParams::builder()
            .with_fee_receiver_factor(self.config.swap_fee_receiver_factor)
            .with_positive_impact_fee_factor(self.config.swap_fee_factor_for_positive_impact)
            .with_negative_impact_fee_factor(self.config.swap_fee_factor_for_negative_impact)
            .build())
    }
}

impl gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn position_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::PositionImpact)
    }

    fn position_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        let config = &self.config;
        PriceImpactParams::builder()
            .with_exponent(config.position_impact_exponent)
            .with_positive_factor(config.position_impact_positive_factor)
            .with_negative_factor(config.position_impact_negative_factor)
            .build()
    }

    fn position_impact_distribution_params(
        &self,
    ) -> gmsol_model::Result<PositionImpactDistributionParams<Self::Num>> {
        let config = &self.config;
        Ok(PositionImpactDistributionParams::builder()
            .distribute_factor(config.position_impact_distribute_factor)
            .min_position_impact_pool_amount(config.min_position_impact_pool_amount)
            .build())
    }

    fn passed_in_seconds_for_position_impact_distribution(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.clocks().price_impact_distribution).passed_in_seconds()
    }
}

impl gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn borrowing_factor_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::BorrowingFactor)
    }

    fn total_borrowing_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::TotalBorrowing)
    }

    fn borrowing_fee_params(&self) -> gmsol_model::Result<BorrowingFeeParams<Self::Num>> {
        Ok(BorrowingFeeParams::builder()
            .receiver_factor(self.config.borrowing_fee_receiver_factor)
            .factor_for_long(self.config.borrowing_fee_factor_for_long)
            .factor_for_short(self.config.borrowing_fee_factor_for_short)
            .exponent_for_long(self.config.borrowing_fee_exponent_for_long)
            .exponent_for_short(self.config.borrowing_fee_exponent_for_short)
            .build())
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.clocks().borrowing).passed_in_seconds()
    }
}

impl gmsol_model::PerpMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn funding_factor_per_second(&self) -> &Self::Signed {
        &self.state().funding_factor_per_second
    }

    fn funding_amount_per_size_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        let kind = if is_long {
            PoolKind::FundingAmountPerSizeForLong
        } else {
            PoolKind::FundingAmountPerSizeForShort
        };
        self.try_pool(kind)
    }

    fn claimable_funding_amount_per_size_pool(
        &self,
        is_long: bool,
    ) -> gmsol_model::Result<&Self::Pool> {
        let kind = if is_long {
            PoolKind::ClaimableFundingAmountPerSizeForLong
        } else {
            PoolKind::ClaimableFundingAmountPerSizeForShort
        };
        self.try_pool(kind)
    }

    fn funding_amount_per_size_adjustment(&self) -> Self::Num {
        constants::FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT
    }

    fn funding_fee_params(&self) -> gmsol_model::Result<FundingFeeParams<Self::Num>> {
        Ok(FundingFeeParams::builder()
            .exponent(self.config.funding_fee_exponent)
            .funding_factor(self.config.funding_fee_factor)
            .max_factor_per_second(self.config.funding_fee_max_factor_per_second)
            .min_factor_per_second(self.config.funding_fee_min_factor_per_second)
            .increase_factor_per_second(self.config.funding_fee_increase_factor_per_second)
            .decrease_factor_per_second(self.config.funding_fee_decrease_factor_per_second)
            .threshold_for_stable_funding(self.config.funding_fee_threshold_for_stable_funding)
            .threshold_for_decrease_funding(self.config.funding_fee_threshold_for_decrease_funding)
            .build())
    }

    fn position_params(&self) -> gmsol_model::Result<PositionParams<Self::Num>> {
        Ok(PositionParams::new(
            self.config.min_position_size_usd,
            self.config.min_collateral_value,
            self.config.min_collateral_factor,
            self.config.max_positive_position_impact_factor,
            self.config.max_negative_position_impact_factor,
            self.config.max_position_impact_factor_for_liquidations,
        ))
    }

    fn order_fee_params(&self) -> gmsol_model::Result<FeeParams<Self::Num>> {
        Ok(FeeParams::builder()
            .with_fee_receiver_factor(self.config.order_fee_receiver_factor)
            .with_positive_impact_fee_factor(self.config.order_fee_factor_for_positive_impact)
            .with_negative_impact_fee_factor(self.config.order_fee_factor_for_negative_impact)
            .build())
    }

    fn open_interest_reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        Ok(self.config.open_interest_reserve_factor)
    }

    fn max_open_interest(&self, is_long: bool) -> gmsol_model::Result<Self::Num> {
        if is_long {
            Ok(self.config.max_open_interest_for_long)
        } else {
            Ok(self.config.max_open_interest_for_short)
        }
    }

    fn min_collateral_factor_for_open_interest_multiplier(
        &self,
        is_long: bool,
    ) -> gmsol_model::Result<Self::Num> {
        if is_long {
            Ok(self
                .config
                .min_collateral_factor_for_open_interest_multiplier_for_long)
        } else {
            Ok(self
                .config
                .min_collateral_factor_for_open_interest_multiplier_for_short)
        }
    }
}

/// As a liquidity market.
pub struct AsLiquidityMarket<'a, M> {
    market: &'a M,
    mint: &'a Mint,
}

impl<'a, M> AsLiquidityMarket<'a, M> {
    /// Create a new [`AsLiquidityMarket`].
    pub fn new(market: &'a M, market_token: &'a Mint) -> Self {
        Self {
            market,
            mint: market_token,
        }
    }
}

impl<'a, M> gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }> for AsLiquidityMarket<'a, M>
where
    M: gmsol_model::BaseMarket<
        { constants::MARKET_DECIMALS },
        Num = Self::Num,
        Signed = Self::Signed,
        Pool = Self::Pool,
    >,
{
    type Num = u128;

    type Signed = i128;

    type Pool = Pool;

    fn liquidity_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.liquidity_pool()
    }

    fn claimable_fee_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.claimable_fee_pool()
    }

    fn swap_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.swap_impact_pool()
    }

    fn open_interest_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.market.open_interest_pool(is_long)
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.market.open_interest_in_tokens_pool(is_long)
    }

    fn collateral_sum_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.market.collateral_sum_pool(is_long)
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        self.market.usd_to_amount_divisor()
    }

    fn max_pool_amount(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        self.market.max_pool_amount(is_long_token)
    }

    fn pnl_factor_config(
        &self,
        kind: gmsol_model::PnlFactorKind,
        is_long: bool,
    ) -> gmsol_model::Result<Self::Num> {
        self.market.pnl_factor_config(kind, is_long)
    }

    fn reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        self.market.reserve_factor()
    }
}

impl<'a, M> gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }>
    for AsLiquidityMarket<'a, M>
where
    M: gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }>,
{
    fn position_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.position_impact_pool()
    }

    fn position_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        self.market.position_impact_params()
    }

    fn position_impact_distribution_params(
        &self,
    ) -> gmsol_model::Result<PositionImpactDistributionParams<Self::Num>> {
        self.market.position_impact_distribution_params()
    }

    fn passed_in_seconds_for_position_impact_distribution(&self) -> gmsol_model::Result<u64> {
        self.market
            .passed_in_seconds_for_position_impact_distribution()
    }
}

impl<'a, M> gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }>
    for AsLiquidityMarket<'a, M>
where
    M: BorrowingFeeMarket<{ constants::MARKET_DECIMALS }>,
{
    fn borrowing_factor_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.borrowing_factor_pool()
    }

    fn total_borrowing_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.market.total_borrowing_pool()
    }

    fn borrowing_fee_params(&self) -> gmsol_model::Result<BorrowingFeeParams<Self::Num>> {
        self.market.borrowing_fee_params()
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        self.market.passed_in_seconds_for_borrowing()
    }
}

impl<'a, M> gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }>
    for AsLiquidityMarket<'a, M>
{
    fn total_supply(&self) -> Self::Num {
        self.mint.supply.into()
    }

    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        self.market.max_pool_value_for_deposit(is_long_token)
    }
}
