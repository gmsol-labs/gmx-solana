/// Price Map.
pub mod price_map;

/// Custom Price Feed.
mod feed;

/// Switchboard.
pub mod switchboard;

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
    constants,
    states::{TokenMapAccess, TokenMapLoader},
    CoreError, CoreResult,
};
use anchor_lang::prelude::*;
use gmsol_utils::{
    oracle::{OracleFlag, MAX_ORACLE_FLAGS},
    price::Decimal,
    token_config::FeedConfig,
};

use self::price_map::PriceMap;
use super::{HasMarketMeta, Seed, Store, TokenConfig, TokenMapHeader, TokenMapRef};

pub use self::{
    chainlink::Chainlink,
    feed::{PriceFeed, PriceFeedPrice},
    pyth::Pyth,
    switchboard::Switchboard,
    time::{ValidateOracleTime, ValidateOracleTimeExt},
    validator::PriceValidator,
};

pub use gmsol_utils::oracle::PriceProviderKind;

gmsol_utils::flags!(OracleFlag, MAX_ORACLE_FLAGS, u8);

/// Oracle Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Oracle {
    version: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 7],
    /// Store.
    pub store: Pubkey,
    /// This address is authorized to **directly** modify
    /// the oracle through instructions.
    pub(crate) authority: Pubkey,
    min_oracle_ts: i64,
    max_oracle_ts: i64,
    min_oracle_slot: u64,
    primary: PriceMap,
    flags: OracleFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 3],
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 256],
}

impl gmsol_utils::InitSpace for Oracle {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for Oracle {
    const SEED: &'static [u8] = b"oracle";
}

impl Oracle {
    /// Initialize the [`Oracle`].
    pub(crate) fn init(&mut self, store: Pubkey, authority: Pubkey) {
        self.clear_all_prices();
        self.store = store;
        self.authority = authority;
    }

    /// Return whether the oracle is cleared.
    pub fn is_cleared(&self) -> bool {
        self.flags.get_flag(OracleFlag::Cleared)
    }

    /// Set prices from remaining accounts.
    pub(crate) fn set_prices_from_remaining_accounts<'info>(
        &mut self,
        mut validator: PriceValidator,
        map: &TokenMapRef,
        tokens: &[Pubkey],
        remaining_accounts: &'info [AccountInfo<'info>],
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

            let oracle_price =
                OraclePrice::parse_from_feed_account(validator.clock(), token_config, feed)?;

            validator.validate_one(
                token_config,
                &oracle_price.provider,
                oracle_price.parts.oracle_ts,
                oracle_price.parts.oracle_slot,
                &oracle_price.parts.price,
                oracle_price.parts.ref_price.as_ref(),
            )?;
            self.primary
                .set(token, oracle_price.parts.price, token_config.is_synthetic())?;
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
            self.flags.set_flag(OracleFlag::Cleared, false);
        }
        Ok(())
    }

    /// Clear all prices.
    pub(crate) fn clear_all_prices(&mut self) {
        self.primary.clear();
        self.min_oracle_ts = i64::MAX;
        self.max_oracle_ts = i64::MIN;
        self.min_oracle_slot = u64::MAX;
        self.flags.set_flag(OracleFlag::Cleared, true);
    }

    #[inline(never)]
    pub(crate) fn with_prices<'info, T>(
        &mut self,
        store: &AccountLoader<'info, Store>,
        token_map: &AccountLoader<'info, TokenMapHeader>,
        tokens: &[Pubkey],
        remaining_accounts: &'info [AccountInfo<'info>],
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
            self.set_prices_from_remaining_accounts(validator, &token_map, tokens, feeds)
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
    pub fn get_primary_price(
        &self,
        token: &Pubkey,
        allow_synthetic: bool,
    ) -> Result<gmsol_model::price::Price<u128>> {
        let price = self
            .primary
            .get(token)
            .ok_or_else(|| error!(CoreError::MissingOraclePrice))?;

        // The `is_synthetic` flag needs to be checked because, for pool tokens,
        // we do not want their token decimals to be manually set (only synthetic
        // tokens are allowed to have their decimals manually configured).
        // This helps prevent the possibility of unit price using incorrect token
        // decimals assumptions.
        //
        // Note: the SPL token mint cannot be closed, and the SPL token 2022 token mint
        // also cannot be closed when there is a supply. Therefore, there is generally
        // no need to worry about the decimals of non-synthetic tokens being changed.
        if !allow_synthetic {
            require!(
                !price.is_synthetic(),
                CoreError::SyntheticTokenPriceIsNotAllowed
            );
        }
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
            index_token_price: self.get_primary_price(&meta.index_token_mint, true)?,
            long_token_price: self.get_primary_price(&meta.long_token_mint, false)?,
            short_token_price: self.get_primary_price(&meta.short_token_mint, false)?,
        };
        Ok(prices)
    }
}

/// Create from program id.
fn from_program_id(program_id: &Pubkey) -> Option<PriceProviderKind> {
    if *program_id == Chainlink::id() {
        Some(PriceProviderKind::Chainlink)
    } else if *program_id == Pyth::id() {
        Some(PriceProviderKind::Pyth)
    } else if *program_id == Switchboard::id() {
        Some(PriceProviderKind::Switchboard)
    } else {
        None
    }
}

struct OraclePrice {
    provider: PriceProviderKind,
    parts: OraclePriceParts,
}

pub(crate) struct OraclePriceParts {
    oracle_slot: u64,
    oracle_ts: i64,
    price: gmsol_utils::Price,
    ref_price: Option<Decimal>,
}

impl OraclePrice {
    fn parse_from_feed_account<'info>(
        clock: &Clock,
        token_config: &TokenConfig,
        account: &'info AccountInfo<'info>,
    ) -> Result<Self> {
        let (provider, parsed) = match from_program_id(account.owner) {
            Some(provider) => (provider, None),
            None if *account.owner == crate::ID => {
                let loader = AccountLoader::<'info, PriceFeed>::try_from(account)?;
                let feed = loader.load()?;
                let kind = feed.provider()?;
                (kind, Some(feed.check_and_get_price(clock, token_config)?))
            }
            None => return Err(error!(CoreError::InvalidPriceFeedAccount)),
        };

        require_eq!(
            token_config.expected_provider().map_err(CoreError::from)?,
            provider
        );

        let feed_config = token_config
            .get_feed_config(&provider)
            .map_err(CoreError::from)?;
        let feed_id = feed_config.feed();

        let mut parts = match provider {
            PriceProviderKind::ChainlinkDataStreams => {
                parsed.ok_or_else(|| error!(CoreError::Internal))?
            }
            PriceProviderKind::Pyth => {
                Pyth::check_and_get_price(clock, token_config, account, feed_id)?
            }
            PriceProviderKind::Chainlink => {
                msg!("[Oracle] Chainlink Data Feeds are no longer supported as of this version");
                return err!(CoreError::Deprecated);
            }
            PriceProviderKind::Switchboard => {
                require_keys_eq!(*feed_id, account.key(), CoreError::InvalidPriceFeedAccount);
                Switchboard::check_and_get_price(clock, token_config, account)?
            }
            kind => {
                msg!("Unsupported price provider: {}", kind);
                return err!(CoreError::Unimplemented);
            }
        };

        if token_config.is_price_adjustment_allowed() {
            let adjusted = try_adjust_price(feed_config, &mut parts)?;
            if adjusted {
                msg!("[Oracle] price is adjusted, feed_id = {}", feed_id);
            }
        }

        Ok(Self { provider, parts })
    }
}

fn try_adjust_price(feed_config: &FeedConfig, parts: &mut OraclePriceParts) -> Result<bool> {
    let Some(factor) = feed_config.max_deviation_factor() else {
        return Ok(false);
    };
    match try_adjust_price_with_max_deviation_factor(
        &factor,
        &parts.price,
        parts.ref_price.as_ref(),
    ) {
        Some(price) => {
            parts.price = price;
            Ok(true)
        }
        None => Ok(false),
    }
}

fn try_adjust_price_with_max_deviation_factor(
    factor: &u128,
    price: &gmsol_utils::Price,
    ref_price: Option<&Decimal>,
) -> Option<gmsol_utils::Price> {
    use gmsol_model::utils::apply_factor;

    let unit_prices = gmsol_model::price::Price::<u128>::from(price);
    let ref_price = match ref_price {
        Some(ref_price) => ref_price.to_unit_price(),
        None => unit_prices.checked_mid()?,
    };
    let max_deviation = apply_factor::<_, { constants::MARKET_DECIMALS }>(&ref_price, factor)?;

    let mut adjusted_price = None;

    if unit_prices.max.abs_diff(ref_price) > max_deviation {
        adjusted_price.get_or_insert(*price).max = price
            .max
            .with_unit_price(ref_price.checked_add(max_deviation)?, false)?;
    }

    if unit_prices.min.abs_diff(ref_price) > max_deviation {
        adjusted_price.get_or_insert(*price).min = price
            .min
            .with_unit_price(ref_price.checked_sub(max_deviation)?, true)?;
    }

    adjusted_price
}
