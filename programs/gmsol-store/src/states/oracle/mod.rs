/// Price Map.
pub mod price_map;

/// Custom Price Feed.
mod feed;

/// Chainlink.
pub mod chainlink;

/// Pyth.
pub mod pyth;

/// Price Validator.
pub mod validator;

/// Oracle time validation.
pub mod time;

use std::ops::Deref;

use crate::{
    states::{TokenMapAccess, TokenMapLoader},
    CoreError, CoreResult,
};
use anchor_lang::{prelude::*, Ids};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use self::price_map::PriceMap;
use super::{HasMarketMeta, Store, TokenConfig, TokenMapHeader, TokenMapRef};

pub use self::{
    chainlink::Chainlink,
    feed::{PriceFeed, PriceFeedPrice},
    pyth::{Pyth, PythLegacy, PYTH_LEGACY_ID},
    time::{ValidateOracleTime, ValidateOracleTimeExt},
    validator::PriceValidator,
};

const MAX_FLAGS: usize = 8;

#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
enum OracleFlag {
    /// Cleared.
    Cleared,
    // CHECK: should have no more than `MAX_FLAGS` of flags.
}

type OracleFlagsMap = bitmaps::Bitmap<MAX_FLAGS>;
type OracleFlagsValue = u8;

/// Oracle Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Oracle {
    pub store: Pubkey,
    min_oracle_ts: i64,
    max_oracle_ts: i64,
    min_oracle_slot: u64,
    primary: PriceMap,
    flags: OracleFlagsValue,
    padding_0: [u8; 3],
}

impl gmsol_utils::InitSpace for Oracle {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Oracle {
    /// The seed for the oracle account's address.
    pub const SEED: &'static [u8] = b"oracle";

    fn get_flag(&self, kind: OracleFlag) -> bool {
        let index = u8::from(kind);
        let map = OracleFlagsMap::from_value(self.flags);
        map.get(usize::from(index))
    }

    fn set_flag(&mut self, kind: OracleFlag, value: bool) -> bool {
        let index = u8::from(kind);
        let mut map = OracleFlagsMap::from_value(self.flags);
        let previous = map.set(usize::from(index), value);
        self.flags = map.into_value();
        previous
    }

    /// Initialize the [`Oracle`].
    pub(crate) fn init(&mut self, store: Pubkey) {
        self.clear_all_prices();
        self.store = store;
    }

    /// Return whether the oracle is cleared.
    pub fn is_cleared(&self) -> bool {
        self.get_flag(OracleFlag::Cleared)
    }

    /// Set prices from remaining accounts.
    pub(crate) fn set_prices_from_remaining_accounts<'info>(
        &mut self,
        mut validator: PriceValidator,
        map: &TokenMapRef,
        tokens: &[Pubkey],
        remaining_accounts: &'info [AccountInfo<'info>],
        chainlink: Option<&Program<'info, Chainlink>>,
    ) -> Result<()> {
        require!(self.is_cleared(), CoreError::PricesAreAlreadySet);
        require!(self.primary.is_empty(), CoreError::PricesAreAlreadySet);
        require!(
            tokens.len() <= PriceMap::MAX_TOKENS,
            CoreError::ExceedMaxLengthLimit
        );
        require!(
            tokens.len() <= remaining_accounts.len(),
            ErrorCode::AccountNotEnoughKeys
        );
        // Assume the remaining accounts are arranged in the following way:
        // [token_config, feed; tokens.len()] [..remaining]
        for (idx, token) in tokens.iter().enumerate() {
            let feed = &remaining_accounts[idx];
            let token_config = map.get(token).ok_or_else(|| error!(CoreError::NotFound))?;

            require!(token_config.is_enabled(), CoreError::TokenConfigDisabled);

            let oracle_price = OraclePrice::parse_from_feed_account(
                validator.clock(),
                token_config,
                chainlink,
                feed,
            )?;

            validator.validate_one(
                token_config,
                &oracle_price.provider,
                oracle_price.oracle_ts,
                oracle_price.oracle_slot,
                &oracle_price.price,
            )?;
            self.primary.set(token, oracle_price.price)?;
        }
        self.update_oracle_ts_and_slot(validator)?;
        Ok(())
    }

    /// Get min oracle slot.
    pub fn min_oracle_slot(&self) -> Option<u64> {
        if self.is_cleared() {
            None
        } else {
            Some(self.min_oracle_slot)
        }
    }

    /// Get min oracle ts.
    pub fn min_oracle_ts(&self) -> i64 {
        self.min_oracle_ts
    }

    /// Get max oracle ts.
    pub fn max_oracle_ts(&self) -> i64 {
        self.max_oracle_ts
    }

    fn update_oracle_ts_and_slot(&mut self, mut validator: PriceValidator) -> Result<()> {
        validator.merge_range(
            self.min_oracle_slot(),
            self.min_oracle_ts,
            self.max_oracle_ts,
        );
        if let Some((min_slot, min_ts, max_ts)) = validator.finish()? {
            self.min_oracle_slot = min_slot;
            self.min_oracle_ts = min_ts;
            self.max_oracle_ts = max_ts;
            self.set_flag(OracleFlag::Cleared, false);
        }
        Ok(())
    }

    /// Clear all prices.
    pub(crate) fn clear_all_prices(&mut self) {
        self.primary.clear();
        self.min_oracle_ts = i64::MAX;
        self.max_oracle_ts = i64::MIN;
        self.min_oracle_slot = u64::MAX;
        self.set_flag(OracleFlag::Cleared, true);
    }

    pub(crate) fn with_prices<'info, T>(
        &mut self,
        store: &AccountLoader<'info, Store>,
        token_map: &AccountLoader<'info, TokenMapHeader>,
        tokens: &[Pubkey],
        remaining_accounts: &'info [AccountInfo<'info>],
        chainlink: Option<&Program<'info, Chainlink>>,
        f: impl FnOnce(&mut Self, &'info [AccountInfo<'info>]) -> Result<T>,
    ) -> Result<T> {
        let validator = PriceValidator::try_from(store.load()?.deref())?;
        require_gte!(
            remaining_accounts.len(),
            tokens.len(),
            CoreError::NotEnoughTokenFeeds,
        );
        let feeds = &remaining_accounts[..tokens.len()];
        let remaining_accounts = &remaining_accounts[tokens.len()..];
        let res = {
            let token_map = token_map.load_token_map()?;
            self.set_prices_from_remaining_accounts(validator, &token_map, tokens, feeds, chainlink)
        };
        match res {
            Ok(()) => {
                let output = f(self, remaining_accounts);
                self.clear_all_prices();
                output
            }
            Err(err) => {
                self.clear_all_prices();
                Err(err)
            }
        }
    }

    /// Validate oracle time.
    pub(crate) fn validate_time(&self, target: &impl ValidateOracleTime) -> CoreResult<()> {
        if self.max_oracle_ts < self.min_oracle_ts {
            msg!("min = {}, max = {}", self.min_oracle_ts, self.max_oracle_ts);
            return Err(CoreError::InvalidOracleTimestampsRange);
        }
        target.validate_min_oracle_slot(self)?;
        target.validate_min_oracle_ts(self)?;
        target.validate_max_oracle_ts(self)?;
        Ok(())
    }

    /// Get primary price for the given token.
    pub(crate) fn get_primary_price(
        &self,
        token: &Pubkey,
    ) -> Result<gmsol_model::price::Price<u128>> {
        let price = self
            .primary
            .get(token)
            .ok_or_else(|| error!(CoreError::MissingOraclePrice))?;
        Ok(gmsol_model::price::Price {
            min: price.min().to_unit_price(),
            max: price.max().to_unit_price(),
        })
    }

    /// Get prices for the market
    pub(crate) fn market_prices(
        &self,
        market: &impl HasMarketMeta,
    ) -> Result<gmsol_model::price::Prices<u128>> {
        let meta = market.market_meta();
        let prices = gmsol_model::price::Prices {
            index_token_price: self.get_primary_price(&meta.index_token_mint)?,
            long_token_price: self.get_primary_price(&meta.long_token_mint)?,
            short_token_price: self.get_primary_price(&meta.short_token_mint)?,
        };
        Ok(prices)
    }
}

/// Price Provider.
pub struct PriceProvider;

pub(crate) static PRICE_PROVIDER_IDS: [Pubkey; 3] = [
    pyth_solana_receiver_sdk::ID,
    chainlink_solana::ID,
    PYTH_LEGACY_ID,
];

impl Ids for PriceProvider {
    fn ids() -> &'static [Pubkey] {
        &PRICE_PROVIDER_IDS
    }
}

/// Supported Price Provider Kind.
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    Default,
    TryFromPrimitive,
    IntoPrimitive,
    PartialEq,
    Eq,
    Hash,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "snake_case"))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[non_exhaustive]
pub enum PriceProviderKind {
    /// Pyth Oracle V2.
    #[default]
    Pyth = 0,
    /// Chainlink Data Feed.
    Chainlink = 1,
    /// Legacy Pyth Push Oracle.
    PythLegacy = 2,
    /// Chainlink Data Streams.
    ChainlinkDataStreams = 3,
}

impl PriceProviderKind {
    /// Create from program id.
    pub fn from_program_id(program_id: &Pubkey) -> Option<Self> {
        if *program_id == Chainlink::id() {
            Some(Self::Chainlink)
        } else if *program_id == Pyth::id() {
            Some(Self::Pyth)
        } else if *program_id == PythLegacy::id() {
            Some(Self::PythLegacy)
        } else {
            None
        }
    }
}

struct OraclePrice {
    provider: PriceProviderKind,
    oracle_slot: u64,
    oracle_ts: i64,
    price: gmsol_utils::Price,
}

impl OraclePrice {
    fn parse_from_feed_account<'info>(
        clock: &Clock,
        token_config: &TokenConfig,
        chainlink: Option<&Program<'info, Chainlink>>,
        account: &'info AccountInfo<'info>,
    ) -> Result<Self> {
        let (provider, parsed) = match PriceProviderKind::from_program_id(account.owner) {
            Some(provider) => (provider, None),
            None if *account.owner == crate::ID => {
                let loader = AccountLoader::<'info, PriceFeed>::try_from(account)?;
                let feed = loader.load()?;
                let kind = feed.provider()?;
                let price = feed.check_and_get_price(clock, token_config)?;
                (
                    kind,
                    Some((feed.last_published_at_slot(), feed.ts(), price)),
                )
            }
            None => return Err(error!(CoreError::InvalidPriceFeedAccount)),
        };

        require_eq!(token_config.expected_provider()?, provider);

        let feed_id = token_config.get_feed(&provider)?;

        let (oracle_slot, oracle_ts, price) = match provider {
            PriceProviderKind::Chainlink => {
                require_eq!(feed_id, account.key(), CoreError::InvalidPriceFeedAccount);
                let program =
                    chainlink.ok_or_else(|| error!(CoreError::ChainlinkProgramIsRequired))?;
                let (oracle_slot, oracle_ts, price) = Chainlink::check_and_get_chainlink_price(
                    clock,
                    program,
                    token_config,
                    account,
                )?;
                (oracle_slot, oracle_ts, price)
            }
            PriceProviderKind::Pyth => {
                let (oracle_slot, oracle_ts, price) =
                    Pyth::check_and_get_price(clock, token_config, account, &feed_id)?;
                (oracle_slot, oracle_ts, price)
            }
            PriceProviderKind::PythLegacy => {
                require_eq!(feed_id, account.key(), CoreError::InvalidPriceFeedAccount);
                // We don't have to check the `feed_id` because the `feed` account is set by the token config keeper.
                let (oracle_slot, oracle_ts, price) =
                    PythLegacy::check_and_get_price(clock, token_config, account)?;
                (oracle_slot, oracle_ts, price)
            }
            PriceProviderKind::ChainlinkDataStreams => {
                parsed.ok_or_else(|| error!(CoreError::Internal))?
            }
        };

        Ok(Self {
            provider,
            oracle_slot,
            oracle_ts,
            price,
        })
    }
}
