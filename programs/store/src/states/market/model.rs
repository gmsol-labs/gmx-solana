use std::ops::Deref;

use anchor_spl::token::Mint;
use gmsol_model::{
    params::{
        fee::{
            BorrowingFeeKinkModelParams, BorrowingFeeKinkModelParamsForOneSide, BorrowingFeeParams,
            FundingFeeParams, LiquidationFeeParams,
        },
        position::PositionImpactDistributionParams,
        FeeParams, PositionParams, PriceImpactParams,
    },
    PoolKind,
};

use crate::constants;

use super::{clock::AsClock, config::MarketConfigFlag, HasMarketMeta, Market, Pool};

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

    fn virtual_inventory_for_swaps_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        match self.virtual_inventory_for_swaps() {
            Some(_) => {
                Err(gmsol_model::Error::InvalidArgument("virtual inventory for the swaps feature is not enabled when the market is used directly"))
            },
            None => {
                Ok(None::<&Self::Pool>)
            }
        }
    }

    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        match self.virtual_inventory_for_positions() {
            Some(_) => {
                Err(gmsol_model::Error::InvalidArgument("virtual inventory for the positions feature is not enabled when the market is used directly"))
            },
            None => {
                Ok(None::<&Self::Pool>)
            }
        }
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
            _ => Err(gmsol_model::Error::InvalidArgument("missing pnl factor")),
        }
    }

    fn reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        Ok(self.config.reserve_factor)
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

    fn ignore_open_interest_for_usage_factor(&self) -> gmsol_model::Result<bool> {
        Ok(self
            .config
            .flag(MarketConfigFlag::IgnoreOpenInterestForUsageFactor))
    }
}

impl gmsol_model::SwapMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn swap_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        Ok(PriceImpactParams::builder()
            .exponent(self.config.swap_impact_exponent)
            .positive_factor(self.config.swap_impact_positive_factor)
            .negative_factor(self.config.swap_impact_negative_factor)
            .build())
    }

    fn swap_fee_params(&self) -> gmsol_model::Result<FeeParams<Self::Num>> {
        Ok(FeeParams::builder()
            .fee_receiver_factor(self.config.swap_fee_receiver_factor)
            .positive_impact_fee_factor(self.config.swap_fee_factor_for_positive_impact)
            .negative_impact_fee_factor(self.config.swap_fee_factor_for_negative_impact)
            .build())
    }
}

impl gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }> for Market {
    fn position_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.try_pool(PoolKind::PositionImpact)
    }

    fn position_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        let config = &self.config;
        Ok(PriceImpactParams::builder()
            .exponent(config.position_impact_exponent)
            .positive_factor(config.position_impact_positive_factor)
            .negative_factor(config.position_impact_negative_factor)
            .build())
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
            .skip_borrowing_fee_for_smaller_side(
                self.config
                    .flag(MarketConfigFlag::SkipBorrowingFeeForSmallerSide),
            )
            .build())
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.clocks().borrowing).passed_in_seconds()
    }

    fn borrowing_fee_kink_model_params(
        &self,
    ) -> gmsol_model::Result<BorrowingFeeKinkModelParams<Self::Num>> {
        Ok(BorrowingFeeKinkModelParams::builder()
            .long(
                BorrowingFeeKinkModelParamsForOneSide::builder()
                    .optimal_usage_factor(self.config.borrowing_fee_optimal_usage_factor_for_long)
                    .base_borrowing_factor(self.config.borrowing_fee_base_factor_for_long)
                    .above_optimal_usage_borrowing_factor(
                        self.config
                            .borrowing_fee_above_optimal_usage_factor_for_long,
                    )
                    .build(),
            )
            .short(
                BorrowingFeeKinkModelParamsForOneSide::builder()
                    .optimal_usage_factor(self.config.borrowing_fee_optimal_usage_factor_for_short)
                    .base_borrowing_factor(self.config.borrowing_fee_base_factor_for_short)
                    .above_optimal_usage_borrowing_factor(
                        self.config
                            .borrowing_fee_above_optimal_usage_factor_for_short,
                    )
                    .build(),
            )
            .build())
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
            .fee_receiver_factor(self.config.order_fee_receiver_factor)
            .positive_impact_fee_factor(self.config.order_fee_factor_for_positive_impact)
            .negative_impact_fee_factor(self.config.order_fee_factor_for_negative_impact)
            .build())
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

    fn liquidation_fee_params(&self) -> gmsol_model::Result<LiquidationFeeParams<Self::Num>> {
        Ok(LiquidationFeeParams::builder()
            .factor(self.config.liquidation_fee_factor)
            .receiver_factor(self.config.liquidation_fee_receiver_factor)
            .build())
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

impl<M> HasMarketMeta for AsLiquidityMarket<'_, M>
where
    M: AsRef<Market>,
{
    fn is_pure(&self) -> bool {
        self.market.as_ref().is_pure()
    }

    fn market_meta(&self) -> &super::MarketMeta {
        self.market.as_ref().market_meta()
    }
}

impl<M> gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }> for AsLiquidityMarket<'_, M>
where
    M: gmsol_model::BaseMarket<
        { constants::MARKET_DECIMALS },
        Num = u128,
        Signed = i128,
        Pool = Pool,
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

    fn virtual_inventory_for_swaps_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        self.market.virtual_inventory_for_swaps_pool()
    }

    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        self.market.virtual_inventory_for_positions_pool()
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

    fn open_interest_reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        self.market.open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> gmsol_model::Result<Self::Num> {
        self.market.max_open_interest(is_long)
    }

    fn ignore_open_interest_for_usage_factor(&self) -> gmsol_model::Result<bool> {
        self.market.ignore_open_interest_for_usage_factor()
    }
}

impl<M> gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }>
    for AsLiquidityMarket<'_, M>
where
    M: gmsol_model::PositionImpactMarket<
        { constants::MARKET_DECIMALS },
        Num = u128,
        Signed = i128,
        Pool = Pool,
    >,
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

impl<M> gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }> for AsLiquidityMarket<'_, M>
where
    M: gmsol_model::BorrowingFeeMarket<
        { constants::MARKET_DECIMALS },
        Num = u128,
        Signed = i128,
        Pool = Pool,
    >,
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

    fn borrowing_fee_kink_model_params(
        &self,
    ) -> gmsol_model::Result<gmsol_model::params::fee::BorrowingFeeKinkModelParams<Self::Num>> {
        self.market.borrowing_fee_kink_model_params()
    }
}

impl<M> gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }> for AsLiquidityMarket<'_, M>
where
    M: gmsol_model::BorrowingFeeMarket<
        { constants::MARKET_DECIMALS },
        Num = u128,
        Signed = i128,
        Pool = Pool,
    >,
    M: gmsol_model::PositionImpactMarket<
        { constants::MARKET_DECIMALS },
        Num = u128,
        Signed = i128,
        Pool = Pool,
    >,
    M: AsRef<Market>,
{
    fn total_supply(&self) -> Self::Num {
        self.mint.supply.into()
    }

    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        self.market
            .as_ref()
            .max_pool_value_for_deposit(is_long_token)
    }
}
