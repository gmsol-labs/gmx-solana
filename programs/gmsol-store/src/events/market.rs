use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use gmsol_model::{
    action::{
        distribute_position_impact::DistributePositionImpactReport,
        update_borrowing_state::UpdateBorrowingReport, update_funding_state::UpdateFundingReport,
    },
    PoolKind,
};

use crate::states::{
    market::{pool::Pool, Clocks},
    OtherState,
};

use super::Event;

/// Market fees updated event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct MarketFeesUpdated {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Position impact distribution report.
    pub position_impact_distribution: DistributePositionImpactReport<u128>,
    /// Update borrowing state report.
    pub update_borrowing_state: UpdateBorrowingReport<u128>,
    /// Update funding state report.
    pub update_funding_state: UpdateFundingReport<u128, i128>,
}

impl gmsol_utils::InitSpace for MarketFeesUpdated {
    const INIT_SPACE: usize = 8
        + 32
        + DistributePositionImpactReport::<u128>::INIT_SPACE
        + UpdateBorrowingReport::<u128>::INIT_SPACE
        + UpdateFundingReport::<u128, i128>::INIT_SPACE;
}

impl Event for MarketFeesUpdated {}

impl MarketFeesUpdated {
    /// Create from reports.
    pub fn from_reports(
        rev: u64,
        market_token: Pubkey,
        position_impact_distribution: DistributePositionImpactReport<u128>,
        update_borrowing_state: UpdateBorrowingReport<u128>,
        update_funding_state: UpdateFundingReport<u128, i128>,
    ) -> Self {
        Self {
            rev,
            market_token,
            position_impact_distribution,
            update_borrowing_state,
            update_funding_state,
        }
    }
}

/// Market borrowing fees updated event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct BorrowingFeesUpdated {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Update borrowing state report.
    pub update_borrowing_state: UpdateBorrowingReport<u128>,
}

impl gmsol_utils::InitSpace for BorrowingFeesUpdated {
    const INIT_SPACE: usize = 8 + 32 + UpdateBorrowingReport::<u128>::INIT_SPACE;
}

impl Event for BorrowingFeesUpdated {}

impl BorrowingFeesUpdated {
    /// Create from report.
    pub fn from_report(
        rev: u64,
        market_token: Pubkey,
        update_borrowing_state: UpdateBorrowingReport<u128>,
    ) -> Self {
        Self {
            rev,
            market_token,
            update_borrowing_state,
        }
    }
}

/// A pool for market.
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace)]
pub struct EventPool {
    /// Whether the pool only contains one kind of token,
    /// i.e. a pure pool.
    /// For a pure pool, only the `long_token_amount` field is used.
    pub is_pure: u8,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding: [u8; 15],
    /// Long token amount.
    pub long_token_amount: u128,
    /// Short token amount.
    pub short_token_amount: u128,
}

static_assertions::const_assert_eq!(EventPool::INIT_SPACE, Pool::INIT_SPACE);

/// Market clocks.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EventClocks {
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding: [u8; 8],
    /// Revision.
    pub rev: u64,
    /// Price impact distribution clock.
    pub price_impact_distribution: i64,
    /// Borrowing clock.
    pub borrowing: i64,
    /// Funding clock.
    pub funding: i64,
    /// ADL updated clock for long.
    pub adl_for_long: i64,
    /// ADL updated clock for short.
    pub adl_for_short: i64,
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) reserved: [i64; 3],
}

static_assertions::const_assert_eq!(EventClocks::INIT_SPACE, Clocks::INIT_SPACE);

/// Market State.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EventOtherState {
    #[cfg_attr(feature = "debug", debug(skip))]
    pub(crate) padding: [u8; 16],
    /// Revision.
    pub rev: u64,
    /// Trade count.
    pub trade_count: u64,
    /// Long token balance.
    pub long_token_balance: u64,
    /// Short token balance.
    pub short_token_balance: u64,
    /// Funding factor per second.
    pub funding_factor_per_second: i128,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    pub(crate) reserved: [u8; 256],
}

static_assertions::const_assert_eq!(EventOtherState::INIT_SPACE, OtherState::INIT_SPACE);

/// Market State Updated Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct MarketStateUpdated {
    /// Revision.
    rev: u64,
    /// Market token.
    market_token: Pubkey,
    /// Updated pool kinds.
    pool_kinds: Vec<PoolKind>,
    /// Updated pools.
    pools: Vec<EventPool>,
    /// Clocks.
    clocks: Vec<EventClocks>,
    /// Other states.
    other: Vec<EventOtherState>,
}

impl MarketStateUpdated {
    const fn space(num_pools: usize, num_clocks: usize, num_other: usize) -> usize {
        8 + 32
            + (4 + PoolKind::INIT_SPACE * num_pools)
            + (4 + Pool::INIT_SPACE * num_pools)
            + (4 + Clocks::INIT_SPACE * num_clocks)
            + (4 + OtherState::INIT_SPACE * num_other)
    }
}

#[cfg(feature = "utils")]
impl MarketStateUpdated {
    /// Get market token.
    pub fn market_token(&self) -> Pubkey {
        self.market_token
    }

    /// Get updated pools.
    pub fn pools(&self) -> impl Iterator<Item = (PoolKind, &EventPool)> {
        self.pool_kinds.iter().copied().zip(self.pools.iter())
    }

    /// Get updated clocks.
    pub fn clocks(&self) -> Option<&EventClocks> {
        self.clocks.first()
    }

    /// Get updated other state.
    pub fn other(&self) -> Option<&EventOtherState> {
        self.other.first()
    }
}

/// This is a cheaper variant of [`MarketStateUpdated`] event, sharing the same format
/// for serialization.
#[derive(BorshSerialize)]
pub(crate) struct MarketStateUpdatedRef<'a> {
    /// Revision.
    rev: u64,
    /// Market token.
    market_token: Pubkey,
    /// Updated pool kinds.
    pool_kinds: Vec<PoolKind>,
    /// Updated pools.
    pools: Vec<&'a Pool>,
    /// Clocks.
    clocks: Vec<&'a Clocks>,
    /// Other states.
    other: Vec<&'a OtherState>,
}

impl<'a> MarketStateUpdatedRef<'a> {
    pub(crate) fn new(
        rev: u64,
        market_token: Pubkey,
        pool_kinds: Vec<PoolKind>,
        pools: Vec<&'a Pool>,
        clocks: Option<&'a Clocks>,
        other: Option<&'a OtherState>,
    ) -> Self {
        assert_eq!(pool_kinds.len(), pools.len());
        Self {
            rev,
            market_token,
            pool_kinds,
            pools,
            clocks: clocks.into_iter().collect(),
            other: other.into_iter().collect(),
        }
    }

    pub(crate) fn space(&self) -> usize {
        MarketStateUpdated::space(self.pools.len(), self.clocks.len(), self.other.len())
    }
}

impl anchor_lang::Discriminator for MarketStateUpdatedRef<'_> {
    const DISCRIMINATOR: &'static [u8] = MarketStateUpdated::DISCRIMINATOR;
}

impl Event for MarketStateUpdatedRef<'_> {}
