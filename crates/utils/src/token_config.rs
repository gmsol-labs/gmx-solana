use std::collections::BTreeSet;

use anchor_lang::prelude::*;

use crate::{
    chunk_by::chunk_by,
    fixed_str::{bytes_to_fixed_str, FixedStrError},
    market::HasMarketMeta,
    oracle::PriceProviderKind,
    pubkey::DEFAULT_PUBKEY,
    swap::HasSwapParams,
};

/// Default heartbeat duration for price updates.
pub const DEFAULT_HEARTBEAT_DURATION: u32 = 30;

/// Default precision for price.
pub const DEFAULT_PRECISION: u8 = 4;

/// Default timestamp adjustment.
pub const DEFAULT_TIMESTAMP_ADJUSTMENT: u32 = 0;

/// Default maximum deviation ratio.
pub const DEFAULT_MAX_DEVIATION_RATIO: u32 = 0;

const MAX_FEEDS: usize = 4;
const MAX_FLAGS: usize = 8;
const MAX_NAME_LEN: usize = 32;

/// Token config error.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TokenConfigError {
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Invalid provider index.
    #[error("invalid provider index")]
    InvalidProviderIndex,
    /// Fixed str error.
    #[error(transparent)]
    FixedStr(#[from] FixedStrError),
    /// Exceed max length limit.
    #[error("exceed max length limit")]
    ExceedMaxLengthLimit,
    /// Exceed max ratio.
    #[error("exceed max ratio")]
    ExceedMaxRatio,
    /// Max deviation factor too small.
    #[error("max deviation factor too small")]
    MaxDeviationFactorTooSmall,
}

pub(crate) type TokenConfigResult<T> = std::result::Result<T, TokenConfigError>;

/// Token Flags.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
#[non_exhaustive]
pub enum TokenConfigFlag {
    /// Is initialized.
    Initialized,
    /// Enabled.
    Enabled,
    /// Is a synthetic asset.
    Synthetic,
    /// Indicates whether price adjustment is allowed.
    AllowPriceAdjustment,
    // CHECK: Cannot have more than `MAX_FLAGS` flags.
}

crate::flags!(TokenConfigFlag, MAX_FLAGS, u8);

#[zero_copy]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokenConfig {
    /// Name.
    pub name: [u8; MAX_NAME_LEN],
    /// Flags.
    pub flags: TokenConfigFlagContainer,
    /// Token decimals.
    pub token_decimals: u8,
    /// Precision.
    pub precision: u8,
    /// Expected provider.
    pub expected_provider: u8,
    /// Price Feeds.
    pub feeds: [FeedConfig; MAX_FEEDS],
    /// Heartbeat duration.
    pub heartbeat_duration: u32,
    #[cfg_attr(feature = "debug", debug(skip))]
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
    pub fn get_feed_config(&self, kind: &PriceProviderKind) -> TokenConfigResult<&FeedConfig> {
        let index = *kind as usize;
        let config = self.feeds.get(index).ok_or(TokenConfigError::NotFound)?;
        if config.feed == DEFAULT_PUBKEY {
            Err(TokenConfigError::NotFound)
        } else {
            Ok(config)
        }
    }

    /// Get the mutable reference of feed config by kind.
    pub fn get_feed_config_mut(
        &mut self,
        kind: &PriceProviderKind,
    ) -> TokenConfigResult<&mut FeedConfig> {
        let index = *kind as usize;
        let config = self
            .feeds
            .get_mut(index)
            .ok_or(TokenConfigError::NotFound)?;
        if config.feed == DEFAULT_PUBKEY {
            Err(TokenConfigError::NotFound)
        } else {
            Ok(config)
        }
    }

    /// Set feed config.
    pub fn set_feed_config(
        &mut self,
        kind: &PriceProviderKind,
        new_config: FeedConfig,
    ) -> TokenConfigResult<()> {
        let index = *kind as usize;
        let config = self
            .feeds
            .get_mut(index)
            .ok_or(TokenConfigError::InvalidProviderIndex)?;
        *config = new_config;
        Ok(())
    }

    /// Get the corresponding price feed address.
    pub fn get_feed(&self, kind: &PriceProviderKind) -> TokenConfigResult<Pubkey> {
        Ok(self.get_feed_config(kind)?.feed)
    }

    /// Set expected provider.
    pub fn set_expected_provider(&mut self, provider: PriceProviderKind) {
        self.expected_provider = provider as u8;
    }

    /// Get expected price provider kind.
    pub fn expected_provider(&self) -> TokenConfigResult<PriceProviderKind> {
        let kind = PriceProviderKind::try_from(self.expected_provider)
            .map_err(|_| TokenConfigError::InvalidProviderIndex)?;
        Ok(kind)
    }

    /// Get price feed address for the expected provider.
    pub fn get_expected_feed(&self) -> TokenConfigResult<Pubkey> {
        self.get_feed(&self.expected_provider()?)
    }

    /// Set enabled.
    pub fn set_enabled(&mut self, enable: bool) {
        self.set_flag(TokenConfigFlag::Enabled, enable)
    }

    /// Set synthetic.
    pub fn set_synthetic(&mut self, is_synthetic: bool) {
        self.set_flag(TokenConfigFlag::Synthetic, is_synthetic)
    }

    /// Is enabled.
    pub fn is_enabled(&self) -> bool {
        self.flag(TokenConfigFlag::Enabled)
    }

    /// Is synthetic.
    pub fn is_synthetic(&self) -> bool {
        self.flag(TokenConfigFlag::Synthetic)
    }

    /// Returns whether the config is a valid pool token config.
    pub fn is_valid_pool_token_config(&self) -> bool {
        !self.is_synthetic()
    }

    /// Returns `true` if price adjustment is allowed.
    pub fn is_price_adjustment_allowed(&self) -> bool {
        self.flag(TokenConfigFlag::AllowPriceAdjustment)
    }

    /// Set flag
    pub fn set_flag(&mut self, flag: TokenConfigFlag, value: bool) {
        self.flags.set_flag(flag, value);
    }

    /// Get flag.
    pub fn flag(&self, flag: TokenConfigFlag) -> bool {
        self.flags.get_flag(flag)
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
    pub fn timestamp_adjustment(
        &self,
        price_provider: &PriceProviderKind,
    ) -> TokenConfigResult<u32> {
        Ok(self.get_feed_config(price_provider)?.timestamp_adjustment())
    }

    /// Get max deviation factor.
    pub fn max_deviation_factor(
        &self,
        price_provider: &PriceProviderKind,
    ) -> TokenConfigResult<Option<u128>> {
        Ok(self.get_feed_config(price_provider)?.max_deviation_factor())
    }

    /// Heartbeat duration.
    pub fn heartbeat_duration(&self) -> u32 {
        self.heartbeat_duration
    }

    /// Get token name.
    pub fn name(&self) -> TokenConfigResult<&str> {
        Ok(bytes_to_fixed_str(&self.name)?)
    }
}

impl crate::InitSpace for TokenConfig {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

/// Price Feed Config.
#[zero_copy]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FeedConfig {
    #[cfg_attr(
        feature = "serde",
        serde(with = "serde_with::As::<serde_with::DisplayFromStr>")
    )]
    feed: Pubkey,
    timestamp_adjustment: u32,
    /// The maximum allowed deviation ratio from the mid-price.
    /// A value of `0` means no restriction is applied.
    max_deviation_ratio: u32,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 24],
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
    /// Multiplier used to convert a `u32` ratio into a `u128` factor.
    /// The resulting precision is `FACTOR_DECIMALS` (typically 20) minus `log(RATIO_MULTIPLIER)`.
    pub const RATIO_MULTIPLIER: u128 = 10u128.pow(12);

    /// Create a new feed config.
    pub fn new(feed: Pubkey) -> Self {
        Self {
            feed,
            timestamp_adjustment: DEFAULT_TIMESTAMP_ADJUSTMENT,
            max_deviation_ratio: DEFAULT_MAX_DEVIATION_RATIO,
            reserved: Default::default(),
        }
    }

    /// Set feed id.
    pub fn with_feed(mut self, feed_id: Pubkey) -> Self {
        self.feed = feed_id;
        self
    }

    /// Set timestamp adjustment,
    pub fn with_timestamp_adjustment(mut self, timestamp_adjustment: u32) -> Self {
        self.timestamp_adjustment = timestamp_adjustment;
        self
    }

    /// Set max deviation factor
    pub fn with_max_deviation_factor(
        mut self,
        max_deviation_factor: Option<u128>,
    ) -> TokenConfigResult<Self> {
        let ratio = match max_deviation_factor {
            Some(factor) => {
                let ratio = (factor / Self::RATIO_MULTIPLIER)
                    .try_into()
                    .map_err(|_| TokenConfigError::ExceedMaxRatio)?;
                if ratio == 0 {
                    return Err(TokenConfigError::MaxDeviationFactorTooSmall);
                }
                ratio
            }
            None => 0,
        };
        self.max_deviation_ratio = ratio;
        Ok(self)
    }

    /// Get feed.
    pub fn feed(&self) -> &Pubkey {
        &self.feed
    }

    /// Get timestamp adjustment.
    pub fn timestamp_adjustment(&self) -> u32 {
        self.timestamp_adjustment
    }

    /// Get max deviation factor.
    pub fn max_deviation_factor(&self) -> Option<u128> {
        let ratio = self.max_deviation_ratio;
        if self.max_deviation_ratio == 0 {
            None
        } else {
            Some(u128::from(ratio) * Self::RATIO_MULTIPLIER)
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct UpdateTokenConfigParams {
    /// Heartbeat duration.
    pub heartbeat_duration: u32,
    /// Price precision.
    pub precision: u8,
    /// Feeds.
    pub feeds: Vec<Pubkey>,
    /// Timestamp adjustments.
    pub timestamp_adjustments: Vec<u32>,
    /// Expected price provider.
    pub expected_provider: Option<u8>,
}

impl Default for UpdateTokenConfigParams {
    fn default() -> Self {
        Self {
            heartbeat_duration: DEFAULT_HEARTBEAT_DURATION,
            precision: DEFAULT_PRECISION,
            feeds: vec![DEFAULT_PUBKEY; MAX_FEEDS],
            timestamp_adjustments: vec![DEFAULT_TIMESTAMP_ADJUSTMENT; MAX_FEEDS],
            expected_provider: None,
        }
    }
}

impl<'a> From<&'a TokenConfig> for UpdateTokenConfigParams {
    fn from(config: &'a TokenConfig) -> Self {
        let (feeds, timestamp_adjustments) = config
            .feeds
            .iter()
            .map(|config| (config.feed, config.timestamp_adjustment))
            .unzip();

        Self {
            heartbeat_duration: config.heartbeat_duration(),
            precision: config.precision(),
            feeds,
            timestamp_adjustments,
            expected_provider: Some(config.expected_provider),
        }
    }
}

impl UpdateTokenConfigParams {
    /// Update the feed address for the given price provider.
    /// Return error when the feed was not set before.
    pub fn update_price_feed(
        mut self,
        kind: &PriceProviderKind,
        new_feed: Pubkey,
        new_timestamp_adjustment: Option<u32>,
    ) -> TokenConfigResult<Self> {
        let index = *kind as usize;
        let feed = self
            .feeds
            .get_mut(index)
            .ok_or(TokenConfigError::NotFound)?;
        let timestamp_adjustment = self
            .timestamp_adjustments
            .get_mut(index)
            .ok_or(TokenConfigError::NotFound)?;
        *feed = new_feed;
        if let Some(new_timestamp_adjustment) = new_timestamp_adjustment {
            *timestamp_adjustment = new_timestamp_adjustment;
        }
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
            require!(self.get(token).is_some(), ErrorCode::RequireViolated);
        }
        tokens.sort_by_cached_key(|token| self.get(token).unwrap().expected_provider);
        Ok(())
    }
}

/// Tokens with feed.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokensWithFeed {
    /// Tokens that require prices,
    /// which must be of the same length with `feeds`.
    pub tokens: Vec<Pubkey>,
    /// Token feeds for the tokens,
    /// which must be of the same length with `tokens`.
    pub feeds: Vec<Pubkey>,
    /// Providers set,
    /// which must be of the same length with `nums`.
    pub providers: Vec<u8>,
    /// The numbers of tokens of each provider.
    pub nums: Vec<u16>,
}

/// A record of token config.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TokenRecord {
    token: Pubkey,
    feed: Pubkey,
    provider: u8,
}

impl TokenRecord {
    /// Create a new [`TokenRecord`]
    pub fn new(token: Pubkey, feed: Pubkey, provider: PriceProviderKind) -> Self {
        Self {
            token,
            feed,
            provider: provider as u8,
        }
    }

    /// Create a new [`TokenRecord`] from token config,
    /// using the expected provider and feed.
    pub fn from_config(token: Pubkey, config: &TokenConfig) -> TokenConfigResult<Self> {
        Ok(Self::new(
            token,
            config.get_expected_feed()?,
            config.expected_provider()?,
        ))
    }
}

impl TokensWithFeed {
    /// Create from token records.
    /// # Panic
    /// Panics if the number of tokens of the same provider exceeds `u16`.
    pub fn try_from_records(mut records: Vec<TokenRecord>) -> TokenConfigResult<Self> {
        records.sort_by_cached_key(|r| r.provider);
        let mut chunks = chunk_by(&records, |a, b| a.provider == b.provider);
        let capacity = chunks.size_hint().0;
        let mut providers = Vec::with_capacity(capacity);
        let mut nums = Vec::with_capacity(capacity);
        chunks.try_for_each(|chunk| {
            providers.push(chunk[0].provider);
            nums.push(
                u16::try_from(chunk.len()).map_err(|_| TokenConfigError::ExceedMaxLengthLimit)?,
            );
            TokenConfigResult::Ok(())
        })?;
        Ok(Self {
            tokens: records.iter().map(|r| r.token).collect(),
            feeds: records.iter().map(|r| r.feed).collect(),
            providers,
            nums,
        })
    }
}

/// Collect token records for the give tokens.
pub fn token_records<A: TokenMapAccess>(
    token_map: &A,
    tokens: &BTreeSet<Pubkey>,
) -> TokenConfigResult<Vec<TokenRecord>> {
    tokens
        .iter()
        .map(|token| {
            let config = token_map.get(token).ok_or(TokenConfigError::NotFound)?;
            TokenRecord::from_config(*token, config)
        })
        .collect::<TokenConfigResult<Vec<_>>>()
}

/// Tokens Collector.
pub struct TokensCollector {
    tokens: Vec<Pubkey>,
}

impl TokensCollector {
    /// Create a new [`TokensCollector`].
    pub fn new(action: Option<&impl HasSwapParams>, extra_capacity: usize) -> Self {
        let mut tokens;
        match action {
            Some(action) => {
                let swap = action.swap();
                tokens = Vec::with_capacity(swap.num_tokens() + extra_capacity);
                // The tokens in the swap params must be sorted.
                tokens.extend_from_slice(swap.tokens());
            }
            None => tokens = Vec::with_capacity(extra_capacity),
        }

        Self { tokens }
    }

    /// Insert a new token.
    pub fn insert_token(&mut self, token: &Pubkey) -> bool {
        match self.tokens.binary_search(token) {
            Ok(_) => false,
            Err(idx) => {
                self.tokens.insert(idx, *token);
                true
            }
        }
    }

    /// Convert to a vec.
    pub fn into_vec(mut self, token_map: &impl TokenMapAccess) -> TokenConfigResult<Vec<Pubkey>> {
        token_map
            .sort_tokens_by_provider(&mut self.tokens)
            .map_err(|_| TokenConfigError::NotFound)?;
        Ok(self.tokens)
    }

    /// Convert to [`TokensWithFeed`].
    pub fn to_feeds(&self, token_map: &impl TokenMapAccess) -> TokenConfigResult<TokensWithFeed> {
        let records = self
            .tokens
            .iter()
            .map(|token| {
                let config = token_map.get(token).ok_or(TokenConfigError::NotFound)?;
                TokenRecord::from_config(*token, config)
            })
            .collect::<TokenConfigResult<Vec<_>>>()?;
        TokensWithFeed::try_from_records(records)
    }
}

/// Max number of treasury token flags.
#[cfg(feature = "treasury")]
pub const MAX_TREASURY_TOKEN_FLAGS: usize = 8;

/// Token Flags.
#[cfg(feature = "treasury")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
#[derive(
    num_enum::IntoPrimitive, Clone, Copy, strum::EnumString, strum::Display, PartialEq, Eq,
)]
#[strum(serialize_all = "snake_case")]
#[repr(u8)]
pub enum TokenFlag {
    /// Allow deposit.
    AllowDeposit,
    /// Allow withdrawal.
    AllowWithdrawal,
    // CHECK: cannot have more than `MAX_TREASURY_TOKEN_FLAGS` flags.
}
