use std::ops::Deref;

use solana_sdk::pubkey::Pubkey;

/// A wrapper for [`Pubkey`], allowing it to be serialized to and deserialized from base58 string.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringPubkey(#[cfg_attr(serde, serde(with = "pubkey"))] pub Pubkey);

impl From<Pubkey> for StringPubkey {
    fn from(value: Pubkey) -> Self {
        Self(value)
    }
}

impl From<StringPubkey> for Pubkey {
    fn from(value: StringPubkey) -> Self {
        value.0
    }
}

impl Deref for StringPubkey {
    type Target = Pubkey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Serialize [`Pubkey`](solana_sdk::pubkey::Pubkey) as string.
#[cfg(serde)]
pub mod pubkey {
    use serde::{Deserialize, Deserializer, Serializer};
    use solana_sdk::pubkey::Pubkey;

    /// Serialize as string.
    pub fn serialize<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&pubkey.to_string())
    }

    /// Deserialize from str.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let pubkey: &str = Deserialize::deserialize(deserializer)?;
        pubkey
            .parse::<Pubkey>()
            .map_err(<D::Error as serde::de::Error>::custom)
    }
}

#[cfg(test)]
mod tests {

    #[cfg(serde)]
    #[test]
    fn string_pubkey() {
        use super::StringPubkey;

        const PUBKEY_STR: &str = r#""1111111QLbz7JHiBTspS962RLKV8GndWFwiEaqKM""#;
        let pubkey: StringPubkey = serde_json::from_str(PUBKEY_STR).unwrap();
        assert_eq!(serde_json::to_string(&pubkey).unwrap(), PUBKEY_STR);
    }
}
