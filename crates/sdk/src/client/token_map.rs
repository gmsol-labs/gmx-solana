use std::sync::Arc;

use bytes::Bytes;
use gmsol_programs::{
    anchor_lang::{self, AccountDeserialize},
    gmsol_store::accounts::TokenMapHeader,
};
use gmsol_utils::{
    dynamic_access,
    token_config::{TokenConfig, TokenMapAccess},
};
use solana_sdk::pubkey::Pubkey;

use crate::utils::zero_copy::{check_discriminator, try_deserialize_unchecked};

/// Token Map.
#[derive(Debug, Clone)]
pub struct TokenMap {
    header: Arc<TokenMapHeader>,
    configs: Bytes,
}

impl TokenMapAccess for TokenMap {
    fn get(&self, token: &Pubkey) -> Option<&TokenConfig> {
        let index = usize::from(*self.header.tokens.get(token)?);
        dynamic_access::get(&self.configs, index)
    }
}

impl TokenMap {
    /// Get the header.
    pub fn header(&self) -> &TokenMapHeader {
        &self.header
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.header.tokens.is_empty()
    }

    /// Get the number of tokens in the map.
    pub fn len(&self) -> usize {
        self.header.tokens.len()
    }

    /// Get all tokens.
    pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.header
            .tokens
            .entries()
            .map(|(k, _)| Pubkey::new_from_array(*k))
    }

    /// Create an iterator over the entires of the map.
    pub fn iter(&self) -> impl Iterator<Item = (Pubkey, &TokenConfig)> + '_ {
        self.tokens()
            .filter_map(|token| self.get(&token).map(|config| (token, config)))
    }
}

impl AccountDeserialize for TokenMap {
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        check_discriminator::<TokenMapHeader>(buf)?;
        Self::try_deserialize_unchecked(buf)
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let header = Arc::new(try_deserialize_unchecked::<TokenMapHeader>(buf)?);
        let (_disc, data) = buf.split_at(8);
        let (_header, configs) = data.split_at(std::mem::size_of::<TokenMapHeader>());
        Ok(Self {
            header,
            configs: Bytes::copy_from_slice(configs),
        })
    }
}
