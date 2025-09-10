use gmsol_utils::{
    oracle::PriceProviderKind,
    token_config::{TokenConfigError, TokenRecord, TokensWithFeed},
};

use super::StringPubkey;

/// Serializable version of [`TokenRecord`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdeTokenRecord {
    /// Token.
    pub token: StringPubkey,
    /// Feed ID.
    pub feed: StringPubkey,
    /// Provider kind.
    pub provider: PriceProviderKind,
}

impl From<SerdeTokenRecord> for TokenRecord {
    fn from(value: SerdeTokenRecord) -> Self {
        Self::new(value.token.0, value.feed.0, value.provider)
    }
}

impl TryFrom<TokenRecord> for SerdeTokenRecord {
    type Error = crate::Error;

    fn try_from(value: TokenRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            token: (*value.token()).into(),
            feed: (*value.feed()).into(),
            provider: value.provider_kind().map_err(crate::Error::custom)?,
        })
    }
}

/// Convert an iterator of [`SerdeTokenRecord`] to [`TokensWithFeed`].
pub fn to_tokens_with_feeds<'a>(
    records: impl IntoIterator<Item = &'a SerdeTokenRecord>,
) -> Result<TokensWithFeed, TokenConfigError> {
    let feeds = records
        .into_iter()
        .map(|record| TokenRecord::from(record.clone()))
        .collect();
    let feeds = TokensWithFeed::try_from_records(feeds)?;
    Ok(feeds)
}
