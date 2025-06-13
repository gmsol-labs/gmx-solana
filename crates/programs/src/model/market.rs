use std::{
    borrow::Borrow,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anchor_lang::prelude::Pubkey;
use bitmaps::Bitmap;
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

use crate::{
    constants,
    gmsol_store::{
        accounts::Market,
        types::{MarketConfig, MarketMeta, Pool, PoolStorage, Pools},
    },
};

use super::clock::{AsClock, AsClockMut};

impl MarketMeta {
    /// Get token side.
    pub fn token_side(&self, token: &Pubkey) -> gmsol_model::Result<bool> {
        if *token == self.long_token_mint {
            Ok(true)
        } else if *token == self.short_token_mint {
            Ok(false)
        } else {
            Err(gmsol_model::Error::InvalidArgument("not a pool token"))
        }
    }
}

impl Pools {
    fn get(&self, kind: PoolKind) -> Option<&PoolStorage> {
        let pool = match kind {
            PoolKind::Primary => &self.primary,
            PoolKind::SwapImpact => &self.swap_impact,
            PoolKind::ClaimableFee => &self.claimable_fee,
            PoolKind::OpenInterestForLong => &self.open_interest_for_long,
            PoolKind::OpenInterestForShort => &self.open_interest_for_short,
            PoolKind::OpenInterestInTokensForLong => &self.open_interest_in_tokens_for_long,
            PoolKind::OpenInterestInTokensForShort => &self.open_interest_in_tokens_for_short,
            PoolKind::PositionImpact => &self.position_impact,
            PoolKind::BorrowingFactor => &self.borrowing_factor,
            PoolKind::FundingAmountPerSizeForLong => &self.funding_amount_per_size_for_long,
            PoolKind::FundingAmountPerSizeForShort => &self.funding_amount_per_size_for_short,
            PoolKind::ClaimableFundingAmountPerSizeForLong => {
                &self.claimable_funding_amount_per_size_for_long
            }
            PoolKind::ClaimableFundingAmountPerSizeForShort => {
                &self.claimable_funding_amount_per_size_for_short
            }
            PoolKind::CollateralSumForLong => &self.collateral_sum_for_long,
            PoolKind::CollateralSumForShort => &self.collateral_sum_for_short,
            PoolKind::TotalBorrowing => &self.total_borrowing,
            _ => return None,
        };
        Some(pool)
    }

    fn get_mut(&mut self, kind: PoolKind) -> Option<&mut PoolStorage> {
        let pool = match kind {
            PoolKind::Primary => &mut self.primary,
            PoolKind::SwapImpact => &mut self.swap_impact,
            PoolKind::ClaimableFee => &mut self.claimable_fee,
            PoolKind::OpenInterestForLong => &mut self.open_interest_for_long,
            PoolKind::OpenInterestForShort => &mut self.open_interest_for_short,
            PoolKind::OpenInterestInTokensForLong => &mut self.open_interest_in_tokens_for_long,
            PoolKind::OpenInterestInTokensForShort => &mut self.open_interest_in_tokens_for_short,
            PoolKind::PositionImpact => &mut self.position_impact,
            PoolKind::BorrowingFactor => &mut self.borrowing_factor,
            PoolKind::FundingAmountPerSizeForLong => &mut self.funding_amount_per_size_for_long,
            PoolKind::FundingAmountPerSizeForShort => &mut self.funding_amount_per_size_for_short,
            PoolKind::ClaimableFundingAmountPerSizeForLong => {
                &mut self.claimable_funding_amount_per_size_for_long
            }
            PoolKind::ClaimableFundingAmountPerSizeForShort => {
                &mut self.claimable_funding_amount_per_size_for_short
            }
            PoolKind::CollateralSumForLong => &mut self.collateral_sum_for_long,
            PoolKind::CollateralSumForShort => &mut self.collateral_sum_for_short,
            PoolKind::TotalBorrowing => &mut self.total_borrowing,
            _ => return None,
        };
        Some(pool)
    }
}

#[repr(u8)]
enum MarketConfigFlag {
    SkipBorrowingFeeForSmallerSide,
    IgnoreOpenInterestForUsageFactor,
}

type MarketConfigFlags = Bitmap<{ constants::NUM_MARKET_CONFIG_FLAGS }>;

impl MarketConfig {
    fn flag(&self, flag: MarketConfigFlag) -> bool {
        MarketConfigFlags::from_value(self.flag.value).get(flag as usize)
    }
}

#[repr(u8)]
#[allow(dead_code)]
enum MarketFlag {
    Enabled,
    Pure,
    AutoDeleveragingEnabledForLong,
    AutoDeleveragingEnabledForShort,
    GTEnabled,
}

type MarketFlags = Bitmap<{ constants::NUM_MARKET_FLAGS }>;

impl Market {
    fn try_pool(&self, kind: PoolKind) -> gmsol_model::Result<&Pool> {
        Ok(&self
            .state
            .pools
            .get(kind)
            .ok_or(gmsol_model::Error::MissingPoolKind(kind))?
            .pool)
    }

    fn try_pool_mut(&mut self, kind: PoolKind) -> gmsol_model::Result<&mut Pool> {
        Ok(&mut self
            .state
            .pools
            .get_mut(kind)
            .ok_or(gmsol_model::Error::MissingPoolKind(kind))?
            .pool)
    }

    fn flag(&self, flag: MarketFlag) -> bool {
        MarketFlags::from_value(self.flags.value).get(flag as usize)
    }
}

/// Market Model.
#[derive(Debug, Clone)]
pub struct MarketModel {
    market: Arc<Market>,
    supply: u64,
}

impl Deref for MarketModel {
    type Target = Market;

    fn deref(&self) -> &Self::Target {
        &self.market
    }
}

impl MarketModel {
    /// Create from parts.
    pub fn from_parts(market: Arc<Market>, supply: u64) -> Self {
        Self { market, supply }
    }

    /// Get whether it is a pure market.
    pub fn is_pure(&self) -> bool {
        self.market.flag(MarketFlag::Pure)
    }

    /// Record transferred in.
    fn record_transferred_in(
        &mut self,
        is_long_token: bool,
        amount: u64,
    ) -> gmsol_model::Result<()> {
        let is_pure = self.market.flag(MarketFlag::Pure);
        let other = &self.market.state.other;

        if is_pure || is_long_token {
            self.make_market_mut().state.other.long_token_balance =
                other.long_token_balance.checked_add(amount).ok_or(
                    gmsol_model::Error::Computation("increasing long token balance"),
                )?;
        } else {
            self.make_market_mut().state.other.short_token_balance =
                other.short_token_balance.checked_add(amount).ok_or(
                    gmsol_model::Error::Computation("increasing short token balance"),
                )?;
        }

        Ok(())
    }

    /// Record transferred out.
    fn record_transferred_out(
        &mut self,
        is_long_token: bool,
        amount: u64,
    ) -> gmsol_model::Result<()> {
        let is_pure = self.market.flag(MarketFlag::Pure);
        let other = &self.market.state.other;

        if is_pure || is_long_token {
            self.make_market_mut().state.other.long_token_balance =
                other.long_token_balance.checked_sub(amount).ok_or(
                    gmsol_model::Error::Computation("decreasing long token balance"),
                )?;
        } else {
            self.make_market_mut().state.other.short_token_balance =
                other.short_token_balance.checked_sub(amount).ok_or(
                    gmsol_model::Error::Computation("decreasing long token balance"),
                )?;
        }

        Ok(())
    }

    fn balance_for_token(&self, is_long_token: bool) -> u64 {
        let other = &self.state.other;
        if is_long_token || self.market.flag(MarketFlag::Pure) {
            other.long_token_balance
        } else {
            other.short_token_balance
        }
    }

    fn make_market_mut(&mut self) -> &mut Market {
        Arc::make_mut(&mut self.market)
    }

    /// Returns the time in seconds since last funding fee state update.
    pub fn passed_in_seconds_for_funding(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.state.clocks.funding).passed_in_seconds()
    }
}

impl gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
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
        Ok(None::<&Self::Pool>)
    }

    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        Ok(None::<&Self::Pool>)
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

impl gmsol_model::SwapMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
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

impl gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
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
        AsClock::from(&self.state.clocks.price_impact_distribution).passed_in_seconds()
    }
}

impl gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
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
        AsClock::from(&self.state.clocks.borrowing).passed_in_seconds()
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

impl gmsol_model::PerpMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn funding_factor_per_second(&self) -> &Self::Signed {
        &self.state.other.funding_factor_per_second
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

impl gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn total_supply(&self) -> Self::Num {
        u128::from(self.supply)
    }

    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        if is_long_token {
            Ok(self.config.max_pool_value_for_deposit_for_long_token)
        } else {
            Ok(self.config.max_pool_value_for_deposit_for_short_token)
        }
    }
}

impl gmsol_model::Bank<Pubkey> for MarketModel {
    type Num = u64;

    fn record_transferred_in_by_token<Q: ?Sized + Borrow<Pubkey>>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        let is_long_token = self.market.meta.token_side(token.borrow())?;
        self.record_transferred_in(is_long_token, *amount)?;
        Ok(())
    }

    fn record_transferred_out_by_token<Q: ?Sized + Borrow<Pubkey>>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        let is_long_token = self.market.meta.token_side(token.borrow())?;
        self.record_transferred_out(is_long_token, *amount)?;
        Ok(())
    }

    fn balance<Q: Borrow<Pubkey> + ?Sized>(&self, token: &Q) -> gmsol_model::Result<Self::Num> {
        let side = self.market.meta.token_side(token.borrow())?;
        Ok(self.balance_for_token(side))
    }
}

impl gmsol_model::BaseMarketMut<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn liquidity_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(PoolKind::Primary)
    }

    fn claimable_fee_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(PoolKind::ClaimableFee)
    }

    fn virtual_inventory_for_swaps_pool_mut(
        &mut self,
    ) -> gmsol_model::Result<Option<impl DerefMut<Target = Self::Pool>>> {
        Ok(None::<&mut Self::Pool>)
    }
}

impl gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn swap_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(PoolKind::SwapImpact)
    }
}

impl gmsol_model::PositionImpactMarketMut<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn position_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut()
            .try_pool_mut(PoolKind::PositionImpact)
    }

    fn just_passed_in_seconds_for_position_impact_distribution(
        &mut self,
    ) -> gmsol_model::Result<u64> {
        AsClockMut::from(
            &mut self
                .make_market_mut()
                .state
                .clocks
                .price_impact_distribution,
        )
        .just_passed_in_seconds()
    }
}

impl gmsol_model::PerpMarketMut<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn just_passed_in_seconds_for_funding(&mut self) -> gmsol_model::Result<u64> {
        AsClockMut::from(&mut self.make_market_mut().state.clocks.funding).just_passed_in_seconds()
    }

    fn funding_factor_per_second_mut(&mut self) -> &mut Self::Signed {
        &mut self.make_market_mut().state.other.funding_factor_per_second
    }

    fn open_interest_pool_mut(&mut self, is_long: bool) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(if is_long {
            PoolKind::OpenInterestForLong
        } else {
            PoolKind::OpenInterestForShort
        })
    }

    fn open_interest_in_tokens_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(if is_long {
            PoolKind::OpenInterestInTokensForLong
        } else {
            PoolKind::OpenInterestInTokensForShort
        })
    }

    fn funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(if is_long {
            PoolKind::FundingAmountPerSizeForLong
        } else {
            PoolKind::FundingAmountPerSizeForShort
        })
    }

    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(if is_long {
            PoolKind::ClaimableFundingAmountPerSizeForLong
        } else {
            PoolKind::ClaimableFundingAmountPerSizeForShort
        })
    }

    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut().try_pool_mut(if is_long {
            PoolKind::CollateralSumForLong
        } else {
            PoolKind::CollateralSumForShort
        })
    }

    fn total_borrowing_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.make_market_mut()
            .try_pool_mut(PoolKind::TotalBorrowing)
    }

    fn virtual_inventory_for_positions_pool_mut(
        &mut self,
    ) -> gmsol_model::Result<Option<impl DerefMut<Target = Self::Pool>>> {
        Ok(None::<&mut Self::Pool>)
    }
}

impl gmsol_model::LiquidityMarketMut<{ constants::MARKET_DECIMALS }> for MarketModel {
    fn mint(&mut self, amount: &Self::Num) -> gmsol_model::Result<()> {
        let new_mint: u64 = (*amount)
            .try_into()
            .map_err(|_| gmsol_model::Error::Overflow)?;
        let new_supply = self
            .supply
            .checked_add(new_mint)
            .ok_or(gmsol_model::Error::Overflow)?;
        self.supply = new_supply;
        Ok(())
    }

    fn burn(&mut self, amount: &Self::Num) -> gmsol_model::Result<()> {
        let new_burn: u64 = (*amount)
            .try_into()
            .map_err(|_| gmsol_model::Error::Overflow)?;
        let new_supply = self
            .supply
            .checked_sub(new_burn)
            .ok_or(gmsol_model::Error::Overflow)?;
        self.supply = new_supply;
        Ok(())
    }
}
