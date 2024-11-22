use std::{borrow::Borrow, cell::RefMut};

use anchor_lang::prelude::*;
use gmsol_model::{
    params::{
        fee::{BorrowingFeeParams, FundingFeeParams},
        position::PositionImpactDistributionParams,
        FeeParams, PositionParams, PriceImpactParams,
    },
    PoolKind,
};

use crate::{
    constants,
    states::{
        market::{
            clock::{AsClock, AsClockMut},
            Clocks,
        },
        Factor, HasMarketMeta, Market, MarketMeta, OtherState, Pool,
    },
    CoreError,
};

use super::Revertible;

/// Revertible Market.
pub struct RevertibleMarket<'a> {
    pub(super) market: RefMut<'a, Market>,
    order_fee_discount_factor: u128,
}

impl<'a, 'info> TryFrom<&'a AccountLoader<'info, Market>> for RevertibleMarket<'a> {
    type Error = Error;

    fn try_from(value: &'a AccountLoader<'info, Market>) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'a> Key for RevertibleMarket<'a> {
    fn key(&self) -> Pubkey {
        self.market.meta.market_token_mint
    }
}

impl<'a> AsRef<Market> for RevertibleMarket<'a> {
    fn as_ref(&self) -> &Market {
        &self.market
    }
}

impl<'a> RevertibleMarket<'a> {
    fn new<'info>(market: &'a AccountLoader<'info, Market>) -> Result<Self> {
        let mut market = market.load_mut()?;
        market.buffer.start_revertible_operation();
        Ok(Self {
            market,
            order_fee_discount_factor: 0,
        })
    }

    pub(crate) fn with_order_fee_discount_factor(mut self, discount: u128) -> Self {
        self.order_fee_discount_factor = discount;
        self
    }

    fn pool(&self, kind: PoolKind) -> gmsol_model::Result<&Pool> {
        let Market { state, buffer, .. } = &*self.market;
        buffer
            .pool(kind, state)
            .ok_or_else(|| gmsol_model::Error::MissingPoolKind(kind))
    }

    fn pool_mut(&mut self, kind: PoolKind) -> gmsol_model::Result<&mut Pool> {
        let Market { state, buffer, .. } = &mut *self.market;
        buffer
            .pool_mut(kind, state)
            .ok_or_else(|| gmsol_model::Error::MissingPoolKind(kind))
    }

    fn other(&self) -> &OtherState {
        let Market { state, buffer, .. } = &*self.market;
        buffer.other(state)
    }

    fn other_mut(&mut self) -> &mut OtherState {
        let Market { state, buffer, .. } = &mut *self.market;
        buffer.other_mut(state)
    }

    fn clocks(&self) -> &Clocks {
        let Market { state, buffer, .. } = &*self.market;
        buffer.clocks(state)
    }

    fn clocks_mut(&mut self) -> &mut Clocks {
        let Market { state, buffer, .. } = &mut *self.market;
        buffer.clocks_mut(state)
    }

    fn balance_for_one_side(&self, is_long: bool) -> u64 {
        let other = self.other();
        if is_long || self.market.is_pure() {
            if self.market.is_pure() {
                other.long_token_balance / 2
            } else {
                other.long_token_balance
            }
        } else {
            other.short_token_balance
        }
    }

    /// Record transferred in.
    fn record_transferred_in(&mut self, is_long_token: bool, amount: u64) -> Result<()> {
        let mint = self.market.meta.market_token_mint;
        let is_pure = self.market.is_pure();
        let other = self.other_mut();

        msg!(
            "[Balance updating] {}: {},{}(+{},{is_long_token})",
            mint,
            other.long_token_balance,
            other.short_token_balance,
            amount,
        );

        if is_pure || is_long_token {
            other.long_token_balance = other
                .long_token_balance
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        } else {
            other.short_token_balance = other
                .short_token_balance
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        }

        msg!(
            "[Balance updated (to be committed)] {}: {},{}",
            mint,
            other.long_token_balance,
            other.short_token_balance
        );
        Ok(())
    }

    /// Record transferred out.
    fn record_transferred_out(&mut self, is_long_token: bool, amount: u64) -> Result<()> {
        let mint = self.market.meta.market_token_mint;
        let is_pure = self.market.is_pure();
        let other = self.other_mut();

        msg!(
            "[Balance updating] {}: {},{}(-{},{is_long_token})",
            mint,
            other.long_token_balance,
            other.short_token_balance,
            amount,
        );

        if is_pure || is_long_token {
            other.long_token_balance = other
                .long_token_balance
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        } else {
            other.short_token_balance = other
                .short_token_balance
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        }

        msg!(
            "[Balance updated (to be committed)] {}: {},{}",
            mint,
            other.long_token_balance,
            other.short_token_balance
        );
        Ok(())
    }

    /// Next trade id.
    ///
    /// This method is idempotent, meaning that multiple calls to it
    /// result in the same state changes as a single call.
    pub(crate) fn next_trade_id(&mut self) -> Result<u64> {
        let next_trade_id = self
            .market
            .state
            .other
            .trade_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.other_mut().trade_count = next_trade_id;
        Ok(next_trade_id)
    }
}

impl<'a> Revertible for RevertibleMarket<'a> {
    fn commit(mut self) {
        let Market {
            meta,
            state,
            buffer,
            ..
        } = &mut *self.market;
        buffer.commit_to_storage(state);
        msg!(
            "[Balance committed] {}: {},{}",
            meta.market_token_mint,
            state.other.long_token_balance,
            state.other.short_token_balance
        );
    }
}

impl<'a> HasMarketMeta for RevertibleMarket<'a> {
    fn is_pure(&self) -> bool {
        self.market.is_pure()
    }
    fn market_meta(&self) -> &MarketMeta {
        self.market.market_meta()
    }
}

impl<'a> gmsol_model::Bank<Pubkey> for RevertibleMarket<'a> {
    type Num = u64;

    fn record_transferred_in_by_token<Q: ?Sized + Borrow<Pubkey>>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        let is_long_token = self.market.meta.to_token_side(token.borrow())?;
        self.record_transferred_in(is_long_token, *amount)?;
        Ok(())
    }

    fn record_transferred_out_by_token<Q: ?Sized + Borrow<Pubkey>>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        let is_long_token = self.market.meta.to_token_side(token.borrow())?;
        self.record_transferred_out(is_long_token, *amount)?;
        Ok(())
    }

    fn balance<Q: Borrow<Pubkey> + ?Sized>(&self, token: &Q) -> gmsol_model::Result<Self::Num> {
        let side = self.market.meta.to_token_side(token.borrow())?;
        Ok(self.balance_for_one_side(side))
    }
}

impl<'a> gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    type Num = u128;

    type Signed = i128;

    type Pool = Pool;

    fn liquidity_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::Primary)
    }

    fn claimable_fee_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::ClaimableFee)
    }

    fn swap_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::SwapImpact)
    }

    fn open_interest_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.pool(if is_long {
            PoolKind::OpenInterestForLong
        } else {
            PoolKind::OpenInterestForShort
        })
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.pool(if is_long {
            PoolKind::OpenInterestInTokensForLong
        } else {
            PoolKind::OpenInterestInTokensForShort
        })
    }

    fn collateral_sum_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.pool(if is_long {
            PoolKind::CollateralSumForLong
        } else {
            PoolKind::CollateralSumForShort
        })
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

impl<'a> gmsol_model::BaseMarketMut<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn liquidity_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::Primary)
    }

    fn claimable_fee_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::ClaimableFee)
    }
}

impl<'a> gmsol_model::SwapMarket<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn swap_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Factor>> {
        self.market.swap_impact_params()
    }

    fn swap_fee_params(&self) -> gmsol_model::Result<FeeParams<Factor>> {
        self.market.swap_fee_params()
    }
}

impl<'a> gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn swap_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::SwapImpact)
    }
}

impl<'a> gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }>
    for RevertibleMarket<'a>
{
    fn position_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::PositionImpact)
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
        AsClock::from(&self.clocks().price_impact_distribution).passed_in_seconds()
    }
}

impl<'a> gmsol_model::PositionImpactMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleMarket<'a>
{
    fn position_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::PositionImpact)
    }

    fn just_passed_in_seconds_for_position_impact_distribution(
        &mut self,
    ) -> gmsol_model::Result<u64> {
        AsClockMut::from(&mut self.clocks_mut().price_impact_distribution).just_passed_in_seconds()
    }
}

impl<'a> gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn borrowing_factor_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::BorrowingFactor)
    }

    fn total_borrowing_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.pool(PoolKind::TotalBorrowing)
    }

    fn borrowing_fee_params(&self) -> gmsol_model::Result<BorrowingFeeParams<Self::Num>> {
        self.market.borrowing_fee_params()
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.clocks().borrowing).passed_in_seconds()
    }
}

impl<'a> gmsol_model::PerpMarket<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn funding_factor_per_second(&self) -> &Self::Signed {
        &self.other().funding_factor_per_second
    }

    fn funding_amount_per_size_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.pool(if is_long {
            PoolKind::FundingAmountPerSizeForLong
        } else {
            PoolKind::FundingAmountPerSizeForShort
        })
    }

    fn claimable_funding_amount_per_size_pool(
        &self,
        is_long: bool,
    ) -> gmsol_model::Result<&Self::Pool> {
        self.pool(if is_long {
            PoolKind::ClaimableFundingAmountPerSizeForLong
        } else {
            PoolKind::ClaimableFundingAmountPerSizeForShort
        })
    }

    fn funding_amount_per_size_adjustment(&self) -> Self::Num {
        self.market.funding_amount_per_size_adjustment()
    }

    fn funding_fee_params(&self) -> gmsol_model::Result<FundingFeeParams<Self::Num>> {
        self.market.funding_fee_params()
    }

    fn position_params(&self) -> gmsol_model::Result<PositionParams<Self::Num>> {
        self.market.position_params()
    }

    fn order_fee_params(&self) -> gmsol_model::Result<FeeParams<Self::Num>> {
        let params = self.market.order_fee_params()?;
        Ok(params.with_discount_factor(self.order_fee_discount_factor))
    }

    fn open_interest_reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        self.market.open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> gmsol_model::Result<Self::Num> {
        self.market.max_open_interest(is_long)
    }

    fn min_collateral_factor_for_open_interest_multiplier(
        &self,
        is_long: bool,
    ) -> gmsol_model::Result<Self::Num> {
        self.market
            .min_collateral_factor_for_open_interest_multiplier(is_long)
    }
}

impl<'a> gmsol_model::PerpMarketMut<{ constants::MARKET_DECIMALS }> for RevertibleMarket<'a> {
    fn just_passed_in_seconds_for_borrowing(&mut self) -> gmsol_model::Result<u64> {
        AsClockMut::from(&mut self.clocks_mut().borrowing).just_passed_in_seconds()
    }

    fn just_passed_in_seconds_for_funding(&mut self) -> gmsol_model::Result<u64> {
        AsClockMut::from(&mut self.clocks_mut().funding).just_passed_in_seconds()
    }

    fn funding_factor_per_second_mut(&mut self) -> &mut Self::Signed {
        &mut self.other_mut().funding_factor_per_second
    }

    fn open_interest_pool_mut(&mut self, is_long: bool) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(if is_long {
            PoolKind::OpenInterestForLong
        } else {
            PoolKind::OpenInterestForShort
        })
    }

    fn open_interest_in_tokens_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(if is_long {
            PoolKind::OpenInterestInTokensForLong
        } else {
            PoolKind::OpenInterestInTokensForShort
        })
    }

    fn borrowing_factor_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::BorrowingFactor)
    }

    fn funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(if is_long {
            PoolKind::FundingAmountPerSizeForLong
        } else {
            PoolKind::FundingAmountPerSizeForShort
        })
    }

    fn claimable_funding_amount_per_size_pool_mut(
        &mut self,
        is_long: bool,
    ) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(if is_long {
            PoolKind::ClaimableFundingAmountPerSizeForLong
        } else {
            PoolKind::ClaimableFundingAmountPerSizeForShort
        })
    }

    fn collateral_sum_pool_mut(&mut self, is_long: bool) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(if is_long {
            PoolKind::CollateralSumForLong
        } else {
            PoolKind::CollateralSumForShort
        })
    }

    fn total_borrowing_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.pool_mut(PoolKind::TotalBorrowing)
    }
}
