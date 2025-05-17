use std::{
    cell::{Ref, RefMut},
    mem::size_of,
};

use anchor_lang::prelude::*;
use gmsol_utils::token_config::{TokenConfigError, TokenConfigFlag};

use crate::{utils::fixed_str::fixed_str_to_bytes, CoreError};

use super::{InitSpace, PriceProviderKind};

pub use gmsol_utils::token_config::{
    FeedConfig, TokenConfig, TokenMapAccess, UpdateTokenConfigParams,
};

/// Default heartbeat duration for price updates.
pub const DEFAULT_HEARTBEAT_DURATION: u32 = 30;

/// Default precision for price.
pub const DEFAULT_PRECISION: u8 = 4;

/// Default timestamp adjustment.
pub const DEFAULT_TIMESTAMP_ADJUSTMENT: u32 = 0;

#[cfg(feature = "utils")]
pub use self::utils::TokenMap;

const MAX_TOKENS: usize = 256;

impl From<TokenConfigError> for CoreError {
    fn from(err: TokenConfigError) -> Self {
        msg!("Token Config Error: {}", err);
        match err {
            TokenConfigError::NotFound => Self::NotFound,
            TokenConfigError::InvalidProviderIndex => Self::InvalidProviderKindIndex,
            TokenConfigError::FixedStr(err) => err.into(),
            TokenConfigError::ExceedMaxLengthLimit => Self::ExceedMaxLengthLimit,
        }
    }
}

pub(crate) trait TokenConfigExt {
    fn update(
        &mut self,
        name: &str,
        synthetic: bool,
        token_decimals: u8,
        builder: UpdateTokenConfigParams,
        enable: bool,
        init: bool,
    ) -> Result<()>;
}

impl TokenConfigExt for TokenConfig {
    fn update(
        &mut self,
        name: &str,
        synthetic: bool,
        token_decimals: u8,
        builder: UpdateTokenConfigParams,
        enable: bool,
        init: bool,
    ) -> Result<()> {
        if init {
            require!(
                !self.flag(TokenConfigFlag::Initialized),
                CoreError::InvalidArgument
            );
            self.set_flag(TokenConfigFlag::Initialized, true);
        } else {
            require!(
                self.flag(TokenConfigFlag::Initialized),
                CoreError::InvalidArgument
            );
            require_eq!(
                self.token_decimals,
                token_decimals,
                CoreError::TokenDecimalsChanged
            );
        }
        let UpdateTokenConfigParams {
            heartbeat_duration,
            precision,
            feeds,
            timestamp_adjustments,
            expected_provider,
        } = builder;

        require_eq!(
            feeds.len(),
            timestamp_adjustments.len(),
            CoreError::InvalidArgument
        );

        self.name = fixed_str_to_bytes(name)?;
        self.set_synthetic(synthetic);
        self.set_enabled(enable);
        self.token_decimals = token_decimals;
        self.precision = precision;
        self.feeds = feeds
            .into_iter()
            .zip(timestamp_adjustments.into_iter())
            .map(|(feed, timestamp_adjustment)| {
                FeedConfig::new(feed).with_timestamp_adjustment(timestamp_adjustment)
            })
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| error!(CoreError::InvalidArgument))?;
        self.expected_provider = expected_provider.unwrap_or(PriceProviderKind::default() as u8);
        self.heartbeat_duration = heartbeat_duration;
        Ok(())
    }
}

gmsol_utils::fixed_map!(
    Tokens,
    Pubkey,
    crate::utils::pubkey::to_bytes,
    u8,
    MAX_TOKENS,
    0
);

/// Header of `TokenMap`.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct TokenMapHeader {
    version: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 7],
    /// The authorized store.
    pub store: Pubkey,
    tokens: Tokens,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 64],
}

impl InitSpace for TokenMapHeader {
    const INIT_SPACE: usize = std::mem::size_of::<TokenMapHeader>();
}

#[cfg(feature = "display")]
impl std::fmt::Display for TokenMapHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TokenMap: store={}, tokens={}",
            self.store,
            self.tokens.len(),
        )
    }
}

impl TokenMapHeader {
    /// Get the space of the whole `TokenMap` required, excluding discriminator.
    pub fn space(num_configs: u8) -> usize {
        TokenMapHeader::INIT_SPACE + (usize::from(num_configs) * TokenConfig::INIT_SPACE)
    }

    /// Get the space after push.
    pub fn space_after_push(&self) -> Result<usize> {
        let num_configs: u8 = self
            .tokens
            .len()
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::ExceedMaxLengthLimit))?
            .try_into()
            .map_err(|_| error!(CoreError::InvalidArgument))?;
        Ok(Self::space(num_configs))
    }

    /// Get tokens.
    pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.tokens
            .entries()
            .map(|(k, _)| Pubkey::new_from_array(*k))
    }

    /// Get the number of tokens.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether this token map is empty.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    fn get_token_config_unchecked<'a>(
        &self,
        token: &Pubkey,
        configs: &'a [u8],
    ) -> Option<&'a TokenConfig> {
        let index = usize::from(*self.tokens.get(token)?);
        crate::utils::dynamic_access::get(configs, index)
    }

    fn get_token_config_mut_unchecked<'a>(
        &self,
        token: &Pubkey,
        configs: &'a mut [u8],
    ) -> Option<&'a mut TokenConfig> {
        let index = usize::from(*self.tokens.get(token)?);
        crate::utils::dynamic_access::get_mut(configs, index)
    }
}

/// Reference to Token Map.
pub struct TokenMapRef<'a> {
    header: Ref<'a, TokenMapHeader>,
    configs: Ref<'a, [u8]>,
}

/// Mutable Reference to Token Map.
pub struct TokenMapMut<'a> {
    header: RefMut<'a, TokenMapHeader>,
    configs: RefMut<'a, [u8]>,
}

/// Token Map Loader.
pub trait TokenMapLoader<'info> {
    /// Load token map.
    fn load_token_map(&self) -> Result<TokenMapRef>;
    /// Load token map with mutable access.
    fn load_token_map_mut(&self) -> Result<TokenMapMut>;
}

impl<'info> TokenMapLoader<'info> for AccountLoader<'info, TokenMapHeader> {
    fn load_token_map(&self) -> Result<TokenMapRef> {
        // Check the account.
        self.load()?;

        let data = self.as_ref().try_borrow_data()?;
        let (_disc, data) = Ref::map_split(data, |d| d.split_at(8));
        let (header, configs) = Ref::map_split(data, |d| d.split_at(size_of::<TokenMapHeader>()));

        Ok(TokenMapRef {
            header: Ref::map(header, bytemuck::from_bytes),
            configs,
        })
    }

    fn load_token_map_mut(&self) -> Result<TokenMapMut> {
        // Check the account for mutablely access.
        self.load_mut()?;

        let data = self.as_ref().try_borrow_mut_data()?;
        let (_disc, data) = RefMut::map_split(data, |d| d.split_at_mut(8));
        let (header, configs) =
            RefMut::map_split(data, |d| d.split_at_mut(size_of::<TokenMapHeader>()));

        Ok(TokenMapMut {
            header: RefMut::map(header, bytemuck::from_bytes_mut),
            configs,
        })
    }
}

impl TokenMapAccess for TokenMapRef<'_> {
    fn get(&self, token: &Pubkey) -> Option<&TokenConfig> {
        self.header.get_token_config_unchecked(token, &self.configs)
    }
}

/// Token Map Operations.
///
/// The token map is append-only.
pub trait TokenMapAccessMut {
    /// Get mutably the config of the given token.
    fn get_mut(&mut self, token: &Pubkey) -> Option<&mut TokenConfig>;

    /// Push a new token config.
    fn push_with(
        &mut self,
        token: &Pubkey,
        f: impl FnOnce(&mut TokenConfig) -> Result<()>,
        new: bool,
    ) -> Result<()>;
}

impl TokenMapAccessMut for TokenMapMut<'_> {
    fn get_mut(&mut self, token: &Pubkey) -> Option<&mut TokenConfig> {
        self.header
            .get_token_config_mut_unchecked(token, &mut self.configs)
    }

    fn push_with(
        &mut self,
        token: &Pubkey,
        f: impl FnOnce(&mut TokenConfig) -> Result<()>,
        new: bool,
    ) -> Result<()> {
        let index = if new {
            let next_index = self.header.tokens.len();
            require!(next_index < MAX_TOKENS, CoreError::ExceedMaxLengthLimit);
            let index = next_index
                .try_into()
                .map_err(|_| error!(CoreError::InvalidArgument))?;
            self.header.tokens.insert_with_options(token, index, true)?;
            index
        } else {
            *self
                .header
                .tokens
                .get(token)
                .ok_or_else(|| error!(CoreError::NotFound))?
        };
        let Some(dst) = crate::utils::dynamic_access::get_mut::<TokenConfig>(
            &mut self.configs,
            usize::from(index),
        ) else {
            return err!(CoreError::NotEnoughSpace);
        };
        (f)(dst)
    }
}

/// Utils for using token map.
#[cfg(feature = "utils")]
pub mod utils {
    use std::sync::Arc;

    use anchor_lang::{prelude::Pubkey, AccountDeserialize};
    use bytes::Bytes;

    use crate::utils::de;

    use super::{TokenConfig, TokenMapAccess, TokenMapHeader};

    /// Token Map.
    pub struct TokenMap {
        header: Arc<TokenMapHeader>,
        configs: Bytes,
    }

    #[cfg(feature = "debug")]
    impl std::fmt::Debug for TokenMap {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TokenMap")
                .field("header", &self.header)
                .field("configs", &self.configs)
                .finish()
        }
    }

    impl TokenMapAccess for TokenMap {
        fn get(&self, token: &Pubkey) -> Option<&TokenConfig> {
            self.header.get_token_config_unchecked(token, &self.configs)
        }
    }

    impl TokenMap {
        /// Get the header.
        pub fn header(&self) -> &TokenMapHeader {
            &self.header
        }

        /// Is empty.
        pub fn is_empty(&self) -> bool {
            self.header.is_empty()
        }

        /// Get the number of tokens in the map.
        pub fn len(&self) -> usize {
            self.header.len()
        }

        /// Get all tokens.
        pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
            self.header.tokens()
        }

        /// Create an iterator over the entires of the map.
        pub fn iter(&self) -> impl Iterator<Item = (Pubkey, &TokenConfig)> + '_ {
            self.tokens()
                .filter_map(|token| self.get(&token).map(|config| (token, config)))
        }
    }

    impl AccountDeserialize for TokenMap {
        fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
            de::check_discriminator::<TokenMapHeader>(buf)?;
            Self::try_deserialize_unchecked(buf)
        }

        fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
            let header = Arc::new(de::try_deserailize_unchecked::<TokenMapHeader>(buf)?);
            let (_disc, data) = buf.split_at(8);
            let (_header, configs) = data.split_at(std::mem::size_of::<TokenMapHeader>());
            Ok(Self {
                header,
                configs: Bytes::copy_from_slice(configs),
            })
        }
    }
}
