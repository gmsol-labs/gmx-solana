use gmsol_model::{ClockKind, PoolKind};
use gmsol_programs::{
    constants::MARKET_DECIMALS,
    gmsol_store::{
        accounts::Market,
        types::{Clocks, MarketConfig, MarketMeta, OtherState, Pool, Pools},
    },
};
use gmsol_utils::market::{MarketConfigKey, MarketFlag};
use indexmap::IndexMap;
use strum::IntoEnumIterator;

use crate::{
    core::token_config::TokenMapAccess,
    utils::{market::MarketDecimals, Amount, Value},
};

use super::StringPubkey;

/// Serializable version of [`Market`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeMarket {
    /// Name.
    pub name: String,
    /// Enabled.
    pub enabled: bool,
    /// Is pure.
    pub is_pure: bool,
    /// Is ADL enabled for long.
    pub is_adl_enabled_for_long: bool,
    /// Is ADL enabled for short.
    pub is_adl_enabled_for_short: bool,
    /// Is GT minting enabled.
    pub is_gt_minting_enabled: bool,
    /// Store address.
    pub store: StringPubkey,
    /// Metadata.
    pub meta: SerdeMarketMeta,
    /// State.
    pub state: SerdeMarketState,
    /// Clocks.
    pub clocks: SerdeMarketClocks,
    /// Pools.
    pub pools: SerdeMarketPools,
    /// Config.
    pub config: SerdeMarketConfig,
}

impl SerdeMarket {
    /// Create from [`Market`].
    pub fn from_market(market: &Market, token_map: &impl TokenMapAccess) -> crate::Result<Self> {
        let flags = &market.flags;
        let decimals = MarketDecimals::new(&market.meta.into(), token_map)?;
        Ok(Self {
            name: market.name()?.to_string(),
            enabled: flags.get_flag(MarketFlag::Enabled),
            is_pure: flags.get_flag(MarketFlag::Pure),
            is_adl_enabled_for_long: flags.get_flag(MarketFlag::AutoDeleveragingEnabledForLong),
            is_adl_enabled_for_short: flags.get_flag(MarketFlag::AutoDeleveragingEnabledForShort),
            is_gt_minting_enabled: flags.get_flag(MarketFlag::GTEnabled),
            store: market.store.into(),
            meta: (&market.meta).into(),
            state: SerdeMarketState::from_other_state(&market.state.other, decimals),
            clocks: (&market.state.clocks).into(),
            pools: SerdeMarketPools::from_pools(&market.state.pools, decimals)?,
            config: (&market.config).into(),
        })
    }
}

/// Serializable version of [`MarketMeta`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeMarketMeta {
    /// Market token address.
    pub market_token: StringPubkey,
    /// Index token address.
    pub index_token: StringPubkey,
    /// Long token address.
    pub long_token: StringPubkey,
    /// Short token address.
    pub short_token: StringPubkey,
}

impl<'a> From<&'a MarketMeta> for SerdeMarketMeta {
    fn from(meta: &'a MarketMeta) -> Self {
        Self {
            market_token: meta.market_token_mint.into(),
            index_token: meta.index_token_mint.into(),
            long_token: meta.long_token_mint.into(),
            short_token: meta.short_token_mint.into(),
        }
    }
}

/// Serializable version of market state.
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeMarketState {
    /// Long token balance.
    pub long_token_balance: Amount,
    /// Short token balance.
    pub short_token_balance: Amount,
    /// Funding factor per second.
    pub funding_factor_per_second: Value,
}

impl SerdeMarketState {
    /// Create from [`OtherState`].
    pub fn from_other_state(state: &OtherState, decimals: MarketDecimals) -> Self {
        let MarketDecimals {
            long_token_decimals,
            short_token_decimals,
            ..
        } = decimals;
        Self {
            long_token_balance: Amount::from_u64(state.long_token_balance, long_token_decimals),
            short_token_balance: Amount::from_u64(state.short_token_balance, short_token_decimals),
            funding_factor_per_second: Value::from_i128(state.funding_factor_per_second),
        }
    }
}

/// Serializable version of [`Clocks`].
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SerdeMarketClocks(pub IndexMap<ClockKind, i64>);

impl<'a> From<&'a Clocks> for SerdeMarketClocks {
    fn from(clocks: &'a Clocks) -> Self {
        let map = ClockKind::iter()
            .filter_map(|kind| clocks.get(kind).map(|clock| (kind, clock)))
            .collect();
        Self(map)
    }
}

/// Serializable version of [`Pool`]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SerdePool {
    /// Whether the pool only contains one kind of token,
    /// i.e. a pure pool.
    /// For a pure pool, only the `long_token_amount` field is used.
    pub is_pure: bool,
    /// Long amount.
    pub long_amount: Amount,
    /// Short amount.
    pub short_amount: Amount,
}

impl SerdePool {
    /// Create from [`Pool`].
    pub fn from_pool(
        pool: &Pool,
        long_amount_decimals: u8,
        short_amount_decimals: u8,
    ) -> crate::Result<Self> {
        Ok(Self {
            is_pure: pool.is_pure(),
            long_amount: Amount::from_u128(pool.long_token_amount, long_amount_decimals)?,
            short_amount: Amount::from_u128(pool.short_token_amount, short_amount_decimals)?,
        })
    }
}

/// Serializable version of [`Pools`].
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SerdeMarketPools(pub IndexMap<PoolKind, SerdePool>);

impl SerdeMarketPools {
    /// Create from [`Pools`].
    pub fn from_pools(pools: &Pools, decimals: MarketDecimals) -> crate::Result<Self> {
        let map = PoolKind::iter()
            .filter_map(|kind| {
                get_pool(pools, kind, decimals)
                    .map(|pool| pool.map(|p| (kind, p)))
                    .transpose()
            })
            .collect::<crate::Result<IndexMap<_, _>>>()?;
        Ok(Self(map))
    }
}

fn get_pool(
    pools: &Pools,
    kind: PoolKind,
    decimals: MarketDecimals,
) -> crate::Result<Option<SerdePool>> {
    let MarketDecimals {
        long_token_decimals,
        short_token_decimals,
        index_token_decimals,
    } = decimals;
    let pool = match kind {
        PoolKind::Primary => SerdePool::from_pool(
            &pools.primary.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::SwapImpact => SerdePool::from_pool(
            &pools.swap_impact.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::ClaimableFee => SerdePool::from_pool(
            &pools.claimable_fee.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::OpenInterestForLong => SerdePool::from_pool(
            &pools.open_interest_for_long.pool,
            MARKET_DECIMALS,
            MARKET_DECIMALS,
        ),
        PoolKind::OpenInterestForShort => SerdePool::from_pool(
            &pools.open_interest_for_short.pool,
            MARKET_DECIMALS,
            MARKET_DECIMALS,
        ),
        PoolKind::OpenInterestInTokensForLong => SerdePool::from_pool(
            &pools.open_interest_in_tokens_for_long.pool,
            index_token_decimals,
            index_token_decimals,
        ),
        PoolKind::OpenInterestInTokensForShort => SerdePool::from_pool(
            &pools.open_interest_in_tokens_for_short.pool,
            index_token_decimals,
            index_token_decimals,
        ),
        PoolKind::PositionImpact => SerdePool::from_pool(
            &pools.position_impact.pool,
            index_token_decimals,
            index_token_decimals,
        ),
        PoolKind::BorrowingFactor => SerdePool::from_pool(
            &pools.borrowing_factor.pool,
            MARKET_DECIMALS,
            MARKET_DECIMALS,
        ),
        PoolKind::FundingAmountPerSizeForLong => unpack_funding_amount_per_size_pool(
            &pools.funding_amount_per_size_for_long.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::FundingAmountPerSizeForShort => unpack_funding_amount_per_size_pool(
            &pools.funding_amount_per_size_for_short.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::ClaimableFundingAmountPerSizeForLong => unpack_funding_amount_per_size_pool(
            &pools.claimable_funding_amount_per_size_for_long.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::ClaimableFundingAmountPerSizeForShort => unpack_funding_amount_per_size_pool(
            &pools.claimable_funding_amount_per_size_for_short.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::CollateralSumForLong => SerdePool::from_pool(
            &pools.collateral_sum_for_long.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::CollateralSumForShort => SerdePool::from_pool(
            &pools.collateral_sum_for_short.pool,
            long_token_decimals,
            short_token_decimals,
        ),
        PoolKind::TotalBorrowing => SerdePool::from_pool(
            &pools.total_borrowing.pool,
            MARKET_DECIMALS,
            MARKET_DECIMALS,
        ),
        _ => return Ok(None),
    };

    Ok(Some(pool?))
}

fn unpack_funding_amount_per_size_pool(
    pool: &Pool,
    long_token_decimals: u8,
    short_token_decimals: u8,
) -> crate::Result<SerdePool> {
    use gmsol_programs::constants::FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT;
    use rust_decimal::{prelude::FromPrimitive, Decimal};

    let adjustment = Decimal::from_i128(FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT as i128).unwrap();
    let mut pool = SerdePool::from_pool(pool, long_token_decimals, short_token_decimals)?;
    pool.long_amount.0 /= adjustment;
    pool.short_amount.0 /= adjustment;

    Ok(pool)
}

/// Serializable version of [`MarketConfig`].
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SerdeMarketConfig(pub IndexMap<MarketConfigKey, Value>);

impl<'a> From<&'a MarketConfig> for SerdeMarketConfig {
    fn from(config: &'a MarketConfig) -> Self {
        let map = MarketConfigKey::iter()
            .filter_map(|key| {
                let factor = config.get(key)?;
                Some((key, Value::from_u128(*factor)))
            })
            .collect();
        Self(map)
    }
}
