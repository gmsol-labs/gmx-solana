use std::{
    cell::{Ref, RefMut},
    mem::size_of,
};

use anchor_lang::prelude::*;

use crate::{
    utils::fixed_str::{bytes_to_fixed_str, fixed_str_to_bytes},
    CoreError,
};

use super::{HasMarketMeta, InitSpace, PriceProviderKind};

/// Default heartbeat duration for price updates.
pub const DEFAULT_HEARTBEAT_DURATION: u32 = 30;

/// Default precision for price.
pub const DEFAULT_PRECISION: u8 = 4;

/// Default timestamp adjustment.
pub const DEFAULT_TIMESTAMP_ADJUSTMENT: u32 = 0;

#[cfg(feature = "utils")]
pub use self::utils::TokenMap;

const MAX_FEEDS: usize = 4;
const MAX_FLAGS: usize = 8;
const MAX_TOKENS: usize = 256;
const MAX_NAME_LEN: usize = 32;

/// Token Flags.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
#[non_exhaustive]
pub enum Flag {
    /// Is initialized.
    Initialized,
    /// Enabled.
    Enabled,
    /// Is a synthetic asset.
    Synthetic,
    // CHECK: Cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(Flag, MAX_FLAGS, u8);

#[zero_copy]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenConfig {
    /// Name.
    name: [u8; MAX_NAME_LEN],
    /// Flags.
    flags: FlagContainer,
    /// Token decimals.
    token_decimals: u8,
    /// Precision.
    precision: u8,
    /// Expected provider.
    expected_provider: u8,
    /// Price Feeds.
    feeds: [FeedConfig; MAX_FEEDS],
    /// Heartbeat duration.
    heartbeat_duration: u32,
    reserved: [u8; 32],
}

#[cfg(feature = "display")]
impl std::fmt::Display for TokenConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Name: {}", self.name().unwrap_or("*unknown*"))?;
        writeln!(f, "Enabled: {}", self.is_enabled())?;
        writeln!(f, "Synthetic: {}", self.is_synthetic())?;
        writeln!(f, "Decimals: {}", self.token_decimals)?;
        writeln!(f, "Precision: {}", self.precision)?;
        writeln!(f, "Heartbeat: {}", self.heartbeat_duration)?;
        writeln!(
            f,
            "Expected Provider: {}",
            self.expected_provider()
                .map(|kind| kind.to_string())
                .unwrap_or("*unknown*".to_string())
        )?;
        Ok(())
    }
}

impl TokenConfig {
    /// Get the corresponding price feed config.
    pub fn get_feed_config(&self, kind: &PriceProviderKind) -> Result<&FeedConfig> {
        let index = *kind as usize;
        let config = self
            .feeds
            .get(index)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        if config.feed == Pubkey::default() {
            err!(CoreError::NotFound)
        } else {
            Ok(config)
        }
    }

    /// Set feed config.
    pub fn set_feed_config(
        &mut self,
        kind: &PriceProviderKind,
        new_config: FeedConfig,
    ) -> Result<()> {
        let index = *kind as usize;
        let config = self
            .feeds
            .get_mut(index)
            .ok_or_else(|| error!(CoreError::InvalidProviderKindIndex))?;
        *config = new_config;
        Ok(())
    }

    /// Get the corresponding price feed address.
    pub fn get_feed(&self, kind: &PriceProviderKind) -> Result<Pubkey> {
        Ok(self.get_feed_config(kind)?.feed)
    }

    /// Set expected provider.
    pub fn set_expected_provider(&mut self, provider: PriceProviderKind) {
        self.expected_provider = provider as u8;
    }

    /// Get expected price provider kind.
    pub fn expected_provider(&self) -> Result<PriceProviderKind> {
        let kind = PriceProviderKind::try_from(self.expected_provider)
            .map_err(|_| CoreError::InvalidProviderKindIndex)?;
        Ok(kind)
    }

    /// Get price feed address for the expected provider.
    pub fn get_expected_feed(&self) -> Result<Pubkey> {
        self.get_feed(&self.expected_provider()?)
    }

    /// Set enabled.
    pub fn set_enabled(&mut self, enable: bool) {
        self.set_flag(Flag::Enabled, enable)
    }

    /// Set synthetic.
    pub fn set_synthetic(&mut self, is_synthetic: bool) {
        self.set_flag(Flag::Synthetic, is_synthetic)
    }

    /// Is enabled.
    pub fn is_enabled(&self) -> bool {
        self.flag(Flag::Enabled)
    }

    /// Is synthetic.
    pub fn is_synthetic(&self) -> bool {
        self.flag(Flag::Synthetic)
    }

    /// Returns whether the config is a valid pool token config.
    pub fn is_valid_pool_token_config(&self) -> bool {
        !self.is_synthetic()
    }

    /// Set flag
    pub fn set_flag(&mut self, flag: Flag, value: bool) {
        self.flags.set_flag(flag, value);
    }

    /// Get flag.
    pub fn flag(&self, flag: Flag) -> bool {
        self.flags.get_flag(flag)
    }

    /// Create a new token config from builder.
    pub fn update(
        &mut self,
        name: &str,
        synthetic: bool,
        token_decimals: u8,
        builder: TokenConfigBuilder,
        enable: bool,
        init: bool,
    ) -> Result<()> {
        if init {
            require!(!self.flag(Flag::Initialized), CoreError::InvalidArgument);
            self.set_flag(Flag::Initialized, true);
        } else {
            require!(self.flag(Flag::Initialized), CoreError::InvalidArgument);
            require_eq!(
                self.token_decimals,
                token_decimals,
                CoreError::TokenDecimalsChanged
            );
        }
        let TokenConfigBuilder {
            heartbeat_duration,
            precision,
            feeds,
            expected_provider,
        } = builder;
        self.name = fixed_str_to_bytes(name)?;
        self.set_synthetic(synthetic);
        self.set_enabled(enable);
        self.token_decimals = token_decimals;
        self.precision = precision;
        self.feeds = feeds
            .into_iter()
            .map(FeedConfig::new)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| error!(CoreError::InvalidArgument))?;
        self.expected_provider = expected_provider.unwrap_or(PriceProviderKind::default() as u8);
        self.heartbeat_duration = heartbeat_duration;
        Ok(())
    }

    /// Token decimals.
    pub fn token_decimals(&self) -> u8 {
        self.token_decimals
    }

    /// Price Precision.
    pub fn precision(&self) -> u8 {
        self.precision
    }

    /// Get timestamp adjustment.
    pub fn timestamp_adjustment(&self, price_provider: &PriceProviderKind) -> Result<u32> {
        Ok(self.get_feed_config(price_provider)?.timestamp_adjustment)
    }

    /// Heartbeat duration.
    pub fn heartbeat_duration(&self) -> u32 {
        self.heartbeat_duration
    }

    /// Get token name.
    pub fn name(&self) -> Result<&str> {
        bytes_to_fixed_str(&self.name)
    }
}

impl InitSpace for TokenConfig {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

/// Price Feed Config.
#[zero_copy]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedConfig {
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    feed: Pubkey,
    timestamp_adjustment: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    reserved: [u8; 28],
}

#[cfg(feature = "display")]
impl std::fmt::Display for FeedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "feed = {}, timestamp_adjustment = {}",
            self.feed, self.timestamp_adjustment
        )
    }
}

impl FeedConfig {
    /// Create a new feed config.
    pub fn new(feed: Pubkey) -> Self {
        Self {
            feed,
            timestamp_adjustment: DEFAULT_TIMESTAMP_ADJUSTMENT,
            reserved: Default::default(),
        }
    }

    /// Change the timestamp adjustment.
    pub fn with_timestamp_adjustment(mut self, timestamp_adjustment: u32) -> Self {
        self.timestamp_adjustment = timestamp_adjustment;
        self
    }

    /// Get feed.
    pub fn feed(&self) -> &Pubkey {
        &self.feed
    }

    /// Get timestamp adjustment.
    pub fn timestamp_adjustment(&self) -> u32 {
        self.timestamp_adjustment
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenConfigBuilder {
    heartbeat_duration: u32,
    precision: u8,
    feeds: Vec<Pubkey>,
    expected_provider: Option<u8>,
}

impl Default for TokenConfigBuilder {
    fn default() -> Self {
        Self {
            heartbeat_duration: DEFAULT_HEARTBEAT_DURATION,
            precision: DEFAULT_PRECISION,
            feeds: vec![Pubkey::default(); MAX_FEEDS],
            expected_provider: None,
        }
    }
}

impl TokenConfigBuilder {
    /// Update the feed address for the given price provider.
    /// Return error when the feed was not set before.
    pub fn update_price_feed(mut self, kind: &PriceProviderKind, new_feed: Pubkey) -> Result<Self> {
        let index = *kind as usize;
        let feed = self
            .feeds
            .get_mut(index)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        *feed = new_feed;
        Ok(self)
    }

    /// Set heartbeat duration.
    pub fn with_heartbeat_duration(mut self, duration: u32) -> Self {
        self.heartbeat_duration = duration;
        self
    }

    /// Set precision.
    pub fn with_precision(mut self, precision: u8) -> Self {
        self.precision = precision;
        self
    }

    /// Set expected provider.
    pub fn with_expected_provider(mut self, provider: PriceProviderKind) -> Self {
        self.expected_provider = Some(provider as u8);
        self
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
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenMapHeader {
    /// The authorized store.
    pub store: Pubkey,
    tokens: Tokens,
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

impl<'a, 'info> TryFrom<Ref<'a, &'info mut [u8]>> for TokenMapRef<'a> {
    type Error = anchor_lang::prelude::Error;

    fn try_from(data: Ref<'a, &'info mut [u8]>) -> std::result::Result<Self, Self::Error> {
        let (_disc, data) = Ref::map_split(data, |d| d.split_at(8));
        let (header, configs) = Ref::map_split(data, |d| d.split_at(size_of::<TokenMapHeader>()));

        Ok(TokenMapRef {
            header: Ref::map(header, bytemuck::from_bytes),
            configs,
        })
    }
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

        self.as_ref().try_borrow_data()?.try_into()
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

/// Read Token Map.
pub trait TokenMapAccess {
    /// Get the config of the given token.
    fn get(&self, token: &Pubkey) -> Option<&TokenConfig>;

    /// Get token configs for the given market.
    ///
    /// Returns the token configs for `index_token`, `long_token` and `short_token`.
    fn token_configs_for_market(&self, market: &impl HasMarketMeta) -> Option<[&TokenConfig; 3]> {
        let meta = market.market_meta();
        let index_token = self.get(&meta.index_token_mint)?;
        let long_token = self.get(&meta.long_token_mint)?;
        let short_token = self.get(&meta.short_token_mint)?;
        Some([index_token, long_token, short_token])
    }

    /// Sort tokens by provider. This sort is stable.
    fn sort_tokens_by_provider(&self, tokens: &mut [Pubkey]) -> Result<()> {
        // Check the existence of token configs.
        for token in tokens.iter() {
            require!(self.get(token).is_some(), CoreError::UnknownOrDisabledToken);
        }
        tokens.sort_by_key(|token| self.get(token).unwrap().expected_provider);
        Ok(())
    }
}

impl<'a> TokenMapAccess for TokenMapRef<'a> {
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

impl<'a> TokenMapAccessMut for TokenMapMut<'a> {
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
