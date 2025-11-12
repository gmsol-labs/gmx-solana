use std::collections::HashMap;

use crate::core::market::MarketConfigKey;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub type DecimalsMap = HashMap<String, u8>;
pub type ValuesMap = HashMap<String, u64>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EncodedRecommendation {
    pub parameter_name: String,
    pub market_address: String,
    #[serde(rename = "new_value")]
    pub new_values: ValuesMap,
    pub timestamp: u64,
    pub reference_id: String,
    pub protocol: String,
    #[serde(default)]
    pub decimals: DecimalsMap,
    pub signature: String,
    pub recovery_id: u8,
}

#[derive(Debug, Clone)]
pub struct PerMarketUpdates {
    pub market: Pubkey,
    pub entries: Vec<(String, u128)>,
}

impl EncodedRecommendation {
    pub fn market_pubkey(&self) -> crate::Result<Pubkey> {
        Pubkey::try_from(self.market_address.as_str()).map_err(crate::Error::custom)
    }
}

pub fn map_key(chaos_key: &str, parameter_name: &str) -> Option<MarketConfigKey> {
    match parameter_name {
        "oiCaps" => match chaos_key {
            "oiCaps/maxOpenInterestForLongs/v1" => Some(MarketConfigKey::MaxOpenInterestForLong),
            "oiCaps/maxOpenInterestForShorts/v1" => Some(MarketConfigKey::MaxOpenInterestForShort),
            _ => None,
        },
        "priceImpact" => match chaos_key {
            "priceImpact/negativePositionImpactFactor/v1" => {
                Some(MarketConfigKey::PositionImpactNegativeFactor)
            }
            "priceImpact/positivePositionImpactFactor/v1" => {
                Some(MarketConfigKey::PositionImpactPositiveFactor)
            }
            "priceImpact/positionImpactExponentFactor/v1" => {
                Some(MarketConfigKey::PositionImpactExponent)
            }
            _ => None,
        },
        _ => None,
    }
}

pub fn to_per_market_updates(
    items: &[EncodedRecommendation],
) -> crate::Result<Vec<PerMarketUpdates>> {
    use std::collections::BTreeMap;

    let mut grouped: BTreeMap<Pubkey, Vec<(String, u128)>> = BTreeMap::new();

    for rec in items {
        let market = rec.market_pubkey()?;
        for (k, v) in &rec.new_values {
            if let Some(dst) = map_key(k, &rec.parameter_name) {
                grouped
                    .entry(market)
                    .or_default()
                    .push((dst.to_string(), (*v).into()));
            }
        }
    }

    Ok(grouped
        .into_iter()
        .map(|(market, entries)| PerMarketUpdates { market, entries })
        .collect())
}
