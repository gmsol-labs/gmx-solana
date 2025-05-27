use gmsol_programs::gmsol_store::{
    accounts::Market,
    types::{MarketMeta, OtherState},
};
use gmsol_utils::market::MarketFlag;

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
    #[cfg_attr(serde, serde(flatten))]
    pub meta: SerdeMarketMeta,
    /// State.
    #[cfg_attr(serde, serde(flatten))]
    pub state: SerdeMarketState,
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
