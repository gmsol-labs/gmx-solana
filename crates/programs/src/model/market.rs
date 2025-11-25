use std::{
    borrow::Borrow,
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anchor_lang::prelude::Pubkey;
use bitmaps::Bitmap;
use bytemuck::Zeroable;
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
        accounts::{Market, Position},
        types::{MarketConfig, MarketMeta, Pool, PoolStorage, Pools},
    },
    model::{position::PositionKind, PositionModel, VirtualInventoryModel},
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
    EnableMarketClosedParams,
    MarketClosedSkipBorrowingFeeForSmallerSide,
}

type MarketConfigFlags = Bitmap<{ constants::NUM_MARKET_CONFIG_FLAGS }>;

impl MarketConfig {
    fn flag(&self, flag: MarketConfigFlag) -> bool {
        MarketConfigFlags::from_value(self.flag.value).get(flag as usize)
    }

    fn use_market_closed_params(&self, is_market_closed: bool) -> bool {
        is_market_closed && self.flag(MarketConfigFlag::EnableMarketClosedParams)
    }

    fn min_collateral_factor_for_liquidation(&self, is_market_closed: bool) -> Option<u128> {
        let factor = if self.use_market_closed_params(is_market_closed) {
            self.market_closed_min_collateral_factor_for_liquidation
        } else {
            self.min_collateral_factor_for_liquidation
        };
        if factor == 0 {
            None
        } else {
            Some(factor)
        }
    }

    fn skip_borrowing_fee_for_smaller_side(&self, is_market_closed: bool) -> bool {
        if self.use_market_closed_params(is_market_closed) {
            self.flag(MarketConfigFlag::MarketClosedSkipBorrowingFeeForSmallerSide)
        } else {
            self.flag(MarketConfigFlag::SkipBorrowingFeeForSmallerSide)
        }
    }

    fn borrowing_fee_base_factor(&self, for_long: bool, is_market_closed: bool) -> u128 {
        match (self.use_market_closed_params(is_market_closed), for_long) {
            (true, _) => self.market_closed_borrowing_fee_base_factor,
            (false, true) => self.borrowing_fee_base_factor_for_long,
            (false, false) => self.borrowing_fee_base_factor_for_short,
        }
    }

    /// Returns above optimal usage borrowing fee factor.
    fn borrowing_fee_above_optimal_usage_factor(
        &self,
        for_long: bool,
        is_market_closed: bool,
    ) -> u128 {
        match (self.use_market_closed_params(is_market_closed), for_long) {
            (true, _) => self.market_closed_borrowing_fee_above_optimal_usage_factor,
            (false, true) => self.borrowing_fee_above_optimal_usage_factor_for_long,
            (false, false) => self.borrowing_fee_above_optimal_usage_factor_for_short,
        }
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
    Closed,
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

    fn is_closed(&self) -> bool {
        self.flag(MarketFlag::Closed)
    }
}

/// Swap Pricing Kind.
#[derive(Debug, Clone, Copy, Default)]
pub enum SwapPricingKind {
    /// Swap.
    #[default]
    Swap,
    /// Deposit.
    Deposit,
    /// Withdrawal.
    Withdrawal,
    /// Shift.
    Shift,
}

/// Market Model.
#[derive(Debug, Clone)]
pub struct MarketModel {
    market: Arc<Market>,
    supply: u64,
    swap_pricing: SwapPricingKind,
    vi_for_swaps: Option<VirtualInventoryModel>,
    vi_for_positions: Option<VirtualInventoryModel>,
    disable_vis: bool,
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
        Self {
            market,
            supply,
            swap_pricing: Default::default(),
            vi_for_swaps: None,
            vi_for_positions: None,
            disable_vis: false,
        }
    }

    /// Get whether it is a pure market.
    pub fn is_pure(&self) -> bool {
        self.market.flag(MarketFlag::Pure)
    }

    /// Get swap pricing kind.
    pub fn swap_pricing(&self) -> &SwapPricingKind {
        &self.swap_pricing
    }

    /// Execute a function with the specified swap pricing kind.
    ///
    /// # Panic Safety
    /// This method uses RAII to ensure state is restored even if the closure panics.
    pub fn with_swap_pricing<T>(
        &mut self,
        swap_pricing: SwapPricingKind,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        struct SwapPricingGuard<'a> {
            model: &'a mut MarketModel,
            original_swap_pricing: SwapPricingKind,
        }

        impl Drop for SwapPricingGuard<'_> {
            fn drop(&mut self) {
                self.model.swap_pricing = self.original_swap_pricing;
            }
        }

        let original_swap_pricing = self.swap_pricing;
        self.swap_pricing = swap_pricing;

        let guard = SwapPricingGuard {
            model: self,
            original_swap_pricing,
        };

        (f)(guard.model)
        // guard is automatically dropped at the end of scope, restoring original state
    }

    /// Execute a function with virtual inventory models from a map.
    ///
    /// This method temporarily replaces the virtual inventory models
    /// (for swaps and positions) of the `MarketModel` with models from the provided map,
    /// executes the provided function, and then restores the original values.
    ///
    /// The virtual inventory models are looked up from the map using the market's
    /// `virtual_inventory_for_swaps` and `virtual_inventory_for_positions` addresses.
    ///
    /// # Arguments
    /// * `vi_map` - A mutable reference to a map of Pubkey to VirtualInventoryModel
    /// * `f` - Function to execute with the temporary VI models
    ///
    /// # Returns
    /// The return value of the function `f`
    ///
    /// # Note
    /// The virtual inventory models are passed via a mutable reference to a map,
    /// allowing the caller to maintain access to the models and observe any
    /// state changes made during the function execution.
    ///
    /// # Panic Safety
    /// This method uses RAII to ensure state is restored even if the closure panics.
    pub fn with_vi_models<T>(
        &mut self,
        vi_map: &mut BTreeMap<Pubkey, VirtualInventoryModel>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        struct ViModelsGuard<'a> {
            model: &'a mut MarketModel,
            loaded_from_map_for_swaps: bool,
            loaded_from_map_for_positions: bool,
        }

        impl Drop for ViModelsGuard<'_> {
            fn drop(&mut self) {
                if self.loaded_from_map_for_swaps {
                    // Detach temporary VI from the model; it will have been
                    // moved back into the map before this guard is dropped.
                    self.model.vi_for_swaps = None;
                }
                if self.loaded_from_map_for_positions {
                    self.model.vi_for_positions = None;
                }
            }
        }

        // Determine the VI addresses, if any.
        let vi_for_swaps_key = (self.market.virtual_inventory_for_swaps != Pubkey::default())
            .then_some(self.market.virtual_inventory_for_swaps);
        let vi_for_positions_key = (self.market.virtual_inventory_for_positions
            != Pubkey::default())
        .then_some(self.market.virtual_inventory_for_positions);

        // Attach VI models from the map to the MarketModel *only if* the model
        // does not already have VI models attached. This ensures that nested
        // calls to `with_vi_models` reuse the same VI instances instead of
        // creating independent copies.
        let mut loaded_from_map_for_swaps = false;
        if self.vi_for_swaps.is_none() {
            if let Some(key) = vi_for_swaps_key {
                if let Some(vi_model) = vi_map.remove(&key) {
                    self.vi_for_swaps = Some(vi_model);
                    loaded_from_map_for_swaps = true;
                }
            }
        }

        let mut loaded_from_map_for_positions = false;
        if self.vi_for_positions.is_none() {
            if let Some(key) = vi_for_positions_key {
                if let Some(vi_model) = vi_map.remove(&key) {
                    self.vi_for_positions = Some(vi_model);
                    loaded_from_map_for_positions = true;
                }
            }
        }

        let guard = ViModelsGuard {
            model: self,
            loaded_from_map_for_swaps,
            loaded_from_map_for_positions,
        };

        let result = (f)(guard.model);

        // Update the map with any changes made to the VI models. For VI models
        // that were taken from the map at the start of this call, move them
        // back into the map so that subsequent users observe the updated state.
        if loaded_from_map_for_swaps {
            if let (Some(key), Some(vi_for_swaps_model)) =
                (vi_for_swaps_key, guard.model.vi_for_swaps.take())
            {
                vi_map.insert(key, vi_for_swaps_model);
            }
        }
        if loaded_from_map_for_positions {
            if let (Some(key), Some(vi_for_positions_model)) =
                (vi_for_positions_key, guard.model.vi_for_positions.take())
            {
                vi_map.insert(key, vi_for_positions_model);
            }
        }

        result
    }

    /// Execute a function with virtual inventories disabled.
    ///
    /// # Panic Safety
    /// This method uses RAII to ensure state is restored even if the closure panics.
    pub fn with_vis_disabled<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        struct DisableVisGuard<'a> {
            model: &'a mut MarketModel,
            original_disable_vis: bool,
        }

        impl Drop for DisableVisGuard<'_> {
            fn drop(&mut self) {
                self.model.disable_vis = self.original_disable_vis;
            }
        }

        let original_disable_vis = self.disable_vis;
        self.disable_vis = true;

        let guard = DisableVisGuard {
            model: self,
            original_disable_vis,
        };

        (f)(guard.model)
        // guard is automatically dropped at the end of scope, restoring original state
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

    /// Check if the market has a virtual inventory address for swaps.
    fn has_vi_for_swaps_address(&self) -> bool {
        self.market.virtual_inventory_for_swaps != Pubkey::default()
    }

    /// Check if the market has a virtual inventory address for positions.
    fn has_vi_for_positions_address(&self) -> bool {
        self.market.virtual_inventory_for_positions != Pubkey::default()
    }

    /// Validate virtual inventory consistency for swaps.
    ///
    /// # Note
    /// We do not validate PDA address matching here because:
    /// - The assumption that the provided VI address must match the calculated PDA is too strong
    /// - PDA address calculation has high computational cost
    fn validate_vi_for_swaps(&self) -> gmsol_model::Result<()> {
        if self.disable_vis {
            return Ok(());
        }

        let market_has_vi = self.has_vi_for_swaps_address();
        let model_has_vi = self.vi_for_swaps.is_some();

        match (market_has_vi, model_has_vi) {
            (true, false) => Err(gmsol_model::Error::InvalidArgument(
                "virtual inventory for swaps should be present but is missing",
            )),
            (false, true) => Err(gmsol_model::Error::InvalidArgument(
                "virtual inventory for swaps should not be present but is provided",
            )),
            _ => Ok(()),
        }
    }

    /// Validate virtual inventory consistency for positions.
    ///
    /// # Note
    /// We do not validate PDA address matching here because:
    /// - The assumption that the provided VI address must match the calculated PDA is too strong
    /// - PDA address calculation has high computational cost
    fn validate_vi_for_positions(&self) -> gmsol_model::Result<()> {
        if self.disable_vis {
            return Ok(());
        }

        let market_has_vi = self.has_vi_for_positions_address();
        let model_has_vi = self.vi_for_positions.is_some();

        match (market_has_vi, model_has_vi) {
            (true, false) => Err(gmsol_model::Error::InvalidArgument(
                "virtual inventory for positions should be present but is missing",
            )),
            (false, true) => Err(gmsol_model::Error::InvalidArgument(
                "virtual inventory for positions should not be present but is provided",
            )),
            _ => Ok(()),
        }
    }

    /// Returns the time in seconds since last funding fee state update.
    pub fn passed_in_seconds_for_funding(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.state.clocks.funding).passed_in_seconds()
    }

    /// Convert into an empty position model.
    ///
    /// # Notes
    /// - All position parameters unrelated to the model,
    ///   such as `owner` and `bump`, use zeroed values.
    pub fn into_empty_position(
        self,
        is_long: bool,
        collateral_token: Pubkey,
    ) -> gmsol_model::Result<PositionModel> {
        self.into_empty_position_opts(is_long, collateral_token, Default::default())
    }

    /// Convert into an empty position model with options.
    pub fn into_empty_position_opts(
        self,
        is_long: bool,
        collateral_token: Pubkey,
        options: PositionOptions,
    ) -> gmsol_model::Result<PositionModel> {
        const POSITION_SEED: &[u8] = b"position";

        if !(self.meta.long_token_mint == collateral_token
            || self.meta.short_token_mint == collateral_token)
        {
            return Err(gmsol_model::Error::InvalidArgument(
                "invalid `collateral_token`",
            ));
        }

        let owner = options.owner.unwrap_or_default();
        let store = &self.store;
        let market_token = &self.meta.market_token_mint;
        let kind = if is_long {
            PositionKind::Long
        } else {
            PositionKind::Short
        } as u8;

        let bump = if options.generate_bump {
            Pubkey::find_program_address(
                &[
                    POSITION_SEED,
                    store.as_ref(),
                    owner.as_ref(),
                    market_token.as_ref(),
                    collateral_token.as_ref(),
                    &[kind],
                ],
                &options.store_program_id,
            )
            .1
        } else {
            0
        };

        let position = Position {
            version: 0,
            bump,
            store: *store,
            kind,
            padding_0: Zeroable::zeroed(),
            created_at: options.created_at,
            owner,
            market_token: *market_token,
            collateral_token,
            state: Zeroable::zeroed(),
            reserved: Zeroable::zeroed(),
        };
        PositionModel::new(self, Arc::new(position))
    }
}

/// Options for creating a position model.
#[derive(Debug, Clone)]
pub struct PositionOptions {
    /// The owner of the position.
    ///
    /// If set to `None`, the `owner` will use the default pubkey.
    pub owner: Option<Pubkey>,
    /// The timestamp of the position creation.
    pub created_at: i64,
    /// Whether to generate a bump seed.
    ///
    /// If set `false`, the `bump` will be fixed to `0`.
    pub generate_bump: bool,
    /// The store program ID used to generate the bump seed.
    pub store_program_id: Pubkey,
}

impl Default for PositionOptions {
    fn default() -> Self {
        Self {
            owner: None,
            created_at: 0,
            generate_bump: false,
            store_program_id: crate::gmsol_store::ID,
        }
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
        if self.disable_vis {
            return Ok(None);
        }
        self.validate_vi_for_swaps()?;
        Ok(self.vi_for_swaps.as_ref().map(|vi| vi.pool()))
    }

    fn virtual_inventory_for_positions_pool(
        &self,
    ) -> gmsol_model::Result<Option<impl Deref<Target = Self::Pool>>> {
        if self.disable_vis {
            return Ok(None);
        }
        self.validate_vi_for_positions()?;
        Ok(self.vi_for_positions.as_ref().map(|vi| vi.pool()))
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
        let params = match self.swap_pricing {
            SwapPricingKind::Shift => FeeParams::builder()
                .fee_receiver_factor(self.config.swap_fee_receiver_factor)
                .positive_impact_fee_factor(0)
                .negative_impact_fee_factor(0)
                .build(),
            SwapPricingKind::Swap | SwapPricingKind::Deposit | SwapPricingKind::Withdrawal => {
                FeeParams::builder()
                    .fee_receiver_factor(self.config.swap_fee_receiver_factor)
                    .positive_impact_fee_factor(self.config.swap_fee_factor_for_positive_impact)
                    .negative_impact_fee_factor(self.config.swap_fee_factor_for_negative_impact)
                    .build()
            }
        };

        Ok(params)
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
                    .skip_borrowing_fee_for_smaller_side(self.is_closed()),
            )
            .build())
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        AsClock::from(&self.state.clocks.borrowing).passed_in_seconds()
    }

    fn borrowing_fee_kink_model_params(
        &self,
    ) -> gmsol_model::Result<BorrowingFeeKinkModelParams<Self::Num>> {
        let is_closed = self.is_closed();
        Ok(BorrowingFeeKinkModelParams::builder()
            .long(
                BorrowingFeeKinkModelParamsForOneSide::builder()
                    .optimal_usage_factor(self.config.borrowing_fee_optimal_usage_factor_for_long)
                    .base_borrowing_factor(self.config.borrowing_fee_base_factor(true, is_closed))
                    .above_optimal_usage_borrowing_factor(
                        self.config
                            .borrowing_fee_above_optimal_usage_factor(true, is_closed),
                    )
                    .build(),
            )
            .short(
                BorrowingFeeKinkModelParamsForOneSide::builder()
                    .optimal_usage_factor(self.config.borrowing_fee_optimal_usage_factor_for_short)
                    .base_borrowing_factor(self.config.borrowing_fee_base_factor(false, is_closed))
                    .above_optimal_usage_borrowing_factor(
                        self.config
                            .borrowing_fee_above_optimal_usage_factor(false, is_closed),
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
        Ok(PositionParams::builder()
            .min_position_size_usd(self.config.min_position_size_usd)
            .min_collateral_value(self.config.min_collateral_value)
            .min_collateral_factor(self.config.min_collateral_factor)
            .max_positive_position_impact_factor(self.config.max_positive_position_impact_factor)
            .max_negative_position_impact_factor(self.config.max_negative_position_impact_factor)
            .max_position_impact_factor_for_liquidations(
                self.config.max_position_impact_factor_for_liquidations,
            )
            .min_collateral_factor_for_liquidation(
                self.config
                    .min_collateral_factor_for_liquidation(self.is_closed()),
            )
            .build())
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
        if self.disable_vis {
            return Ok(None);
        }
        self.validate_vi_for_swaps()?;
        Ok(self.vi_for_swaps.as_mut().map(|vi| vi.pool_mut()))
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
        if self.disable_vis {
            return Ok(None);
        }
        self.validate_vi_for_positions()?;
        Ok(self.vi_for_positions.as_mut().map(|vi| vi.pool_mut()))
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
