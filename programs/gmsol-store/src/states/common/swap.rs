use std::collections::{BTreeSet, HashSet};

use anchor_lang::prelude::*;

use crate::{
    states::{HasMarketMeta, Market, TokenMapAccess},
    CoreError,
};

use super::{TokenRecord, TokensWithFeed};

const MAX_STEPS: usize = 10;
const MAX_TOKENS: usize = 2 * MAX_STEPS + 2 + 3;
const MAX_FLAGS: usize = 8;

/// Swap params.
#[zero_copy]
#[derive(Default)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SwapParams {
    /// The length of primary swap path.
    primary_length: u8,
    /// The length of secondary swap path.
    secondary_length: u8,
    /// The number of tokens.
    num_tokens: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 1],
    current_market_token: Pubkey,
    /// Swap paths.
    paths: [Pubkey; MAX_STEPS],
    /// Tokens.
    tokens: [Pubkey; MAX_TOKENS],
}

impl SwapParams {
    /// Max total length of swap paths.
    pub const MAX_TOTAL_LENGTH: usize = MAX_STEPS;

    /// Max total number of tokens of swap path.
    pub const MAX_TOKENS: usize = MAX_TOKENS;

    /// Get the length of primary swap path.
    pub fn primary_length(&self) -> usize {
        usize::from(self.primary_length)
    }

    /// Get the length of secondary swap path.
    pub fn secondary_length(&self) -> usize {
        usize::from(self.secondary_length)
    }

    /// Get the number of tokens.
    pub fn num_tokens(&self) -> usize {
        usize::from(self.num_tokens)
    }

    /// Get primary swap path.
    pub fn primary_swap_path(&self) -> &[Pubkey] {
        let end = self.primary_length();
        &self.paths[0..end]
    }

    /// Get secondary swap path.
    pub fn secondary_swap_path(&self) -> &[Pubkey] {
        let start = self.primary_length();
        let end = start.saturating_add(self.secondary_length());
        &self.paths[start..end]
    }

    /// Get validated primary swap path.
    pub fn validated_primary_swap_path(&self) -> Result<&[Pubkey]> {
        let mut seen: HashSet<&Pubkey> = HashSet::default();
        require!(
            self.primary_swap_path()
                .iter()
                .all(move |token| seen.insert(token)),
            CoreError::InvalidSwapPath
        );
        Ok(self.primary_swap_path())
    }

    /// Get validated secondary swap path.
    pub fn validated_secondary_swap_path(&self) -> Result<&[Pubkey]> {
        let mut seen: HashSet<&Pubkey> = HashSet::default();
        require!(
            self.secondary_swap_path()
                .iter()
                .all(move |token| seen.insert(token)),
            CoreError::InvalidSwapPath
        );
        Ok(self.secondary_swap_path())
    }

    /// Get all tokens for the action.
    pub fn tokens(&self) -> &[Pubkey] {
        let end = self.num_tokens();
        &self.tokens[0..end]
    }

    /// Convert to token records.
    pub fn to_token_records<'a>(
        &'a self,
        map: &'a impl TokenMapAccess,
    ) -> impl Iterator<Item = Result<TokenRecord>> + 'a {
        self.tokens().iter().map(|token| {
            let config = map
                .get(token)
                .ok_or_else(|| error!(CoreError::UnknownToken))?;
            TokenRecord::from_config(*token, config)
        })
    }

    /// Convert to tokens with feed.
    pub fn to_feeds(&self, map: &impl TokenMapAccess) -> Result<TokensWithFeed> {
        let records = self.to_token_records(map).collect::<Result<Vec<_>>>()?;
        TokensWithFeed::try_from_records(records)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_and_init<'info>(
        &mut self,
        current_market: &impl HasMarketMeta,
        primary_length: u8,
        secondary_length: u8,
        paths: &'info [AccountInfo<'info>],
        store: &Pubkey,
        token_ins: (&Pubkey, &Pubkey),
        token_outs: (&Pubkey, &Pubkey),
        extension: &mut SwapParamsExtension,
    ) -> Result<()> {
        require!(!extension.is_enabled(), CoreError::PreconditionsAreNotMet);

        let primary_end = usize::from(primary_length);
        let end = primary_end.saturating_add(usize::from(secondary_length));
        require_gte!(
            Self::MAX_TOTAL_LENGTH,
            end,
            CoreError::InvalidSwapPathLength
        );

        require_gte!(paths.len(), end, CoreError::NotEnoughSwapMarkets);
        let primary_markets = &paths[..primary_end];
        let secondary_markets = &paths[primary_end..end];

        let (primary_token_in, secondary_token_in) = token_ins;
        let (primary_token_out, secondary_token_out) = token_outs;

        let meta = current_market.market_meta();
        let mut tokens = BTreeSet::from([
            meta.index_token_mint,
            meta.long_token_mint,
            meta.short_token_mint,
        ]);
        let primary_path = validate_path(
            &mut tokens,
            primary_markets,
            store,
            primary_token_in,
            primary_token_out,
        )?;
        let secondary_path = validate_path(
            &mut tokens,
            secondary_markets,
            store,
            secondary_token_in,
            secondary_token_out,
        )?;

        require_gte!(Self::MAX_TOKENS, tokens.len(), CoreError::InvalidSwapPath);

        self.primary_length = primary_length;
        self.secondary_length = secondary_length;
        self.num_tokens = tokens.len() as u8;

        for (idx, (market_token, bump)) in
            primary_path.iter().chain(secondary_path.iter()).enumerate()
        {
            self.paths[idx] = *market_token;
            extension.bumps[idx] = *bump;
        }

        for (idx, token) in tokens.into_iter().enumerate() {
            self.tokens[idx] = token;
        }

        self.current_market_token = meta.market_token_mint;
        extension
            .flags
            .set_flag(SwapParamsExtensionFlag::Enabled, true);

        Ok(())
    }

    /// Iterate over both swap paths, primary path first then secondary path.
    pub fn iter(&self) -> impl Iterator<Item = &Pubkey> {
        self.primary_swap_path()
            .iter()
            .chain(self.secondary_swap_path().iter())
    }

    /// Get unique market tokens excluding current market token.
    pub fn unique_market_tokens_excluding_current<'a>(
        &'a self,
        current_market_token: &'a Pubkey,
    ) -> impl Iterator<Item = &'a Pubkey> + 'a {
        let mut seen = HashSet::from([current_market_token]);
        self.iter().filter(move |token| seen.insert(token))
    }

    /// Unpack markets for swap.
    pub fn unpack_markets_for_swap<'info>(
        &self,
        current_market_token: &Pubkey,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Vec<AccountLoader<'info, Market>>> {
        let len = self
            .unique_market_tokens_excluding_current(current_market_token)
            .count();
        require_gte!(
            remaining_accounts.len(),
            len,
            ErrorCode::AccountNotEnoughKeys
        );
        let loaders = unpack_markets(remaining_accounts).collect::<Result<Vec<_>>>()?;
        Ok(loaders)
    }

    /// Find first market.
    fn find_first_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
        extension: &SwapParamsExtension,
    ) -> Result<Option<&'info AccountInfo<'info>>> {
        let Some(MarketAddresses {
            address: target,
            token: first_market_token,
        }) = extension.find_market_address_by_index(self, store, is_primary, Some(0))?
        else {
            return Ok(None);
        };

        let is_current_market = first_market_token == self.current_market_token;

        match remaining_accounts.iter().find(|info| *info.key == target) {
            Some(info) => Ok(Some(info)),
            None if is_current_market => Ok(None),
            None => err!(CoreError::NotFound),
        }
    }

    /// Find first market and unpack.
    pub fn find_and_unpack_first_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
        extension: &SwapParamsExtension,
    ) -> Result<Option<AccountLoader<'info, Market>>> {
        let Some(info) =
            self.find_first_market(store, is_primary, remaining_accounts, extension)?
        else {
            return Ok(None);
        };
        let market = AccountLoader::<Market>::try_from(info)?;
        require_keys_eq!(market.load()?.store, *store, CoreError::StoreMismatched);
        Ok(Some(market))
    }

    /// Find last market.
    fn find_last_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
        extension: &SwapParamsExtension,
    ) -> Result<Option<&'info AccountInfo<'info>>> {
        let Some(MarketAddresses {
            address: target,
            token: last_market_token,
        }) = extension.find_market_address_by_index(self, store, is_primary, None)?
        else {
            return Ok(None);
        };

        let is_current_market = last_market_token == self.current_market_token;

        match remaining_accounts.iter().find(|info| *info.key == target) {
            Some(info) => Ok(Some(info)),
            None if is_current_market => Ok(None),
            None => err!(CoreError::NotFound),
        }
    }

    /// Find last market and unpack.
    pub fn find_and_unpack_last_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
        extension: &SwapParamsExtension,
    ) -> Result<Option<AccountLoader<'info, Market>>> {
        let Some(info) = self.find_last_market(store, is_primary, remaining_accounts, extension)?
        else {
            return Ok(None);
        };
        let market = AccountLoader::<Market>::try_from(info)?;
        require_keys_eq!(market.load()?.store, *store, CoreError::StoreMismatched);
        Ok(Some(market))
    }

    /// Get the first market token in the swap path.
    pub fn first_market_token(&self, is_primary: bool) -> Option<&Pubkey> {
        if is_primary {
            self.primary_swap_path().first()
        } else {
            self.secondary_swap_path().first()
        }
    }

    /// Get the last market token in the swap path.
    pub fn last_market_token(&self, is_primary: bool) -> Option<&Pubkey> {
        if is_primary {
            self.primary_swap_path().last()
        } else {
            self.secondary_swap_path().last()
        }
    }
}

/// Swap params extension.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SwapParamsExtension {
    /// Flags.
    flags: SwapParamsExtensionFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 5],
    /// Bump seeds of markets.
    bumps: [u8; MAX_STEPS],
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 48],
}

impl SwapParamsExtension {
    fn is_enabled(&self) -> bool {
        self.flags.get_flag(SwapParamsExtensionFlag::Enabled)
    }

    fn primary_bumps(&self, params: &SwapParams) -> &[u8] {
        let end = params.primary_length();
        &self.bumps[0..end]
    }

    fn secondary_bumps(&self, params: &SwapParams) -> &[u8] {
        let start = params.primary_length();
        let end = start.saturating_add(params.secondary_length());
        &self.bumps[0..end]
    }

    /// Find market addresses by index.
    ///
    /// Return last market addresses if `index` is `None`.
    fn find_market_address_by_index(
        &self,
        params: &SwapParams,
        store: &Pubkey,
        is_primary: bool,
        index: Option<usize>,
    ) -> Result<Option<MarketAddresses>> {
        let (path, bumps) = if is_primary {
            (params.primary_swap_path(), self.primary_bumps(params))
        } else {
            (params.secondary_swap_path(), self.secondary_bumps(params))
        };

        debug_assert_eq!(path.len(), bumps.len());

        let index = match index {
            Some(index) => index,
            None => {
                let len = path.len();
                if len == 0 {
                    return Ok(None);
                } else {
                    len - 1
                }
            }
        };

        let Some(market_token) = path.get(index) else {
            return Ok(None);
        };

        let address = if self.is_enabled() {
            let Some(bump) = bumps.get(index) else {
                return err!(CoreError::Internal);
            };

            Market::create_market_address(store, market_token, &crate::ID, *bump)
                .map_err(|_| error!(CoreError::Internal))?
        } else {
            Market::find_market_address(store, market_token, &crate::ID).0
        };

        Ok(Some(MarketAddresses {
            address,
            token: *market_token,
        }))
    }
}

struct MarketAddresses {
    address: Pubkey,
    token: Pubkey,
}

/// Flags for [`SwapParamsExtension`].
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
#[non_exhaustive]
pub enum SwapParamsExtensionFlag {
    /// Whether the extension is enabled.
    Enabled,
    // CHECK: Cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(SwapParamsExtensionFlag, MAX_FLAGS, u8);

pub(crate) fn unpack_markets<'info>(
    path: &'info [AccountInfo<'info>],
) -> impl Iterator<Item = Result<AccountLoader<'info, Market>>> {
    path.iter().map(AccountLoader::try_from)
}

fn validate_path<'info>(
    tokens: &mut BTreeSet<Pubkey>,
    path: &'info [AccountInfo<'info>],
    store: &Pubkey,
    token_in: &Pubkey,
    token_out: &Pubkey,
) -> Result<Vec<(Pubkey, u8)>> {
    let mut current = *token_in;
    let mut seen = HashSet::<_>::default();

    let mut validated_market_tokens = Vec::with_capacity(path.len());
    for market in unpack_markets(path) {
        let market = market?;

        if !seen.insert(market.key()) {
            return err!(CoreError::InvalidSwapPath);
        }

        let market = market.load()?;
        let meta = market.validated_meta(store)?;
        if current == meta.long_token_mint {
            current = meta.short_token_mint;
        } else if current == meta.short_token_mint {
            current = meta.long_token_mint
        } else {
            return err!(CoreError::InvalidSwapPath);
        }
        tokens.insert(meta.index_token_mint);
        tokens.insert(meta.long_token_mint);
        tokens.insert(meta.short_token_mint);
        validated_market_tokens.push((meta.market_token_mint, market.bump));
    }

    require_keys_eq!(current, *token_out, CoreError::InvalidSwapPath);

    Ok(validated_market_tokens)
}

/// Has swap parameters.
pub trait HasSwapParams {
    /// Get the swap params.
    fn swap(&self) -> &SwapParams;

    /// Get the swap params extension.
    fn swap_extension(&self) -> &SwapParamsExtension;

    /// Run a function with the swap params.
    fn with_swap_params<T>(&self, f: impl FnOnce(&SwapParams, &SwapParamsExtension) -> T) -> T {
        (f)(self.swap(), self.swap_extension())
    }
}
