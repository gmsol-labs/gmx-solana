use std::collections::{BTreeSet, HashSet};

use anchor_lang::prelude::*;
use gmsol_utils::swap::SwapActionParamsError;

use crate::{
    states::{HasMarketMeta, Market},
    CoreError,
};

pub use gmsol_utils::swap::{HasSwapParams, SwapActionParams};

/// Extension trait for [`SwapActionParams`].
pub(crate) trait SwapActionParamsExt {
    fn validate_and_init<'info>(
        &mut self,
        current_market: &impl HasMarketMeta,
        primary_length: u8,
        secondary_length: u8,
        paths: &'info [AccountInfo<'info>],
        store: &Pubkey,
        token_ins: (&Pubkey, &Pubkey),
        token_outs: (&Pubkey, &Pubkey),
    ) -> Result<()>;

    fn unpack_markets_for_swap<'info>(
        &self,
        current_market_token: &Pubkey,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<(
        Vec<AccountLoader<'info, Market>>,
        &'info [AccountInfo<'info>],
    )>;

    fn find_first_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<&'info AccountInfo<'info>>>;

    fn find_and_unpack_first_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<AccountLoader<'info, Market>>> {
        let Some(info) = self.find_first_market(store, is_primary, remaining_accounts)? else {
            return Ok(None);
        };
        let market = AccountLoader::<Market>::try_from(info)?;
        require_keys_eq!(market.load()?.store, *store, CoreError::StoreMismatched);
        Ok(Some(market))
    }

    fn find_last_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<&'info AccountInfo<'info>>>;

    fn find_and_unpack_last_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<AccountLoader<'info, Market>>> {
        let Some(info) = self.find_last_market(store, is_primary, remaining_accounts)? else {
            return Ok(None);
        };
        let market = AccountLoader::<Market>::try_from(info)?;
        require_keys_eq!(market.load()?.store, *store, CoreError::StoreMismatched);
        Ok(Some(market))
    }
}

impl SwapActionParamsExt for SwapActionParams {
    fn validate_and_init<'info>(
        &mut self,
        current_market: &impl HasMarketMeta,
        primary_length: u8,
        secondary_length: u8,
        paths: &'info [AccountInfo<'info>],
        store: &Pubkey,
        token_ins: (&Pubkey, &Pubkey),
        token_outs: (&Pubkey, &Pubkey),
    ) -> Result<()> {
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

        for (idx, market_token) in primary_path.iter().chain(secondary_path.iter()).enumerate() {
            self.paths[idx] = *market_token;
        }

        for (idx, token) in tokens.into_iter().enumerate() {
            self.tokens[idx] = token;
        }

        self.current_market_token = meta.market_token_mint;

        Ok(())
    }

    fn unpack_markets_for_swap<'info>(
        &self,
        current_market_token: &Pubkey,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<(
        Vec<AccountLoader<'info, Market>>,
        &'info [AccountInfo<'info>],
    )> {
        let len = self
            .unique_market_tokens_excluding_current(current_market_token)
            .count();
        require_gte!(
            remaining_accounts.len(),
            len,
            ErrorCode::AccountNotEnoughKeys
        );
        let (remaining_accounts_for_swap, remaining_accounts) = remaining_accounts.split_at(len);
        let loaders = unpack_markets(remaining_accounts_for_swap).collect::<Result<Vec<_>>>()?;
        Ok((loaders, remaining_accounts))
    }

    fn find_first_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<&'info AccountInfo<'info>>> {
        let path = if is_primary {
            self.primary_swap_path()
        } else {
            self.secondary_swap_path()
        };
        let Some(first_market_token) = path.first() else {
            return Ok(None);
        };
        let is_current_market = *first_market_token == self.current_market_token;
        let target = Market::find_market_address(store, first_market_token, &crate::ID).0;

        match remaining_accounts.iter().find(|info| *info.key == target) {
            Some(info) => Ok(Some(info)),
            None if is_current_market => Ok(None),
            None => err!(CoreError::NotFound),
        }
    }

    fn find_last_market<'info>(
        &self,
        store: &Pubkey,
        is_primary: bool,
        remaining_accounts: &'info [AccountInfo<'info>],
    ) -> Result<Option<&'info AccountInfo<'info>>> {
        let path = if is_primary {
            self.primary_swap_path()
        } else {
            self.secondary_swap_path()
        };
        let Some(last_market_token) = path.last() else {
            return Ok(None);
        };
        let is_current_market = *last_market_token == self.current_market_token;
        let target = Market::find_market_address(store, last_market_token, &crate::ID).0;

        match remaining_accounts.iter().find(|info| *info.key == target) {
            Some(info) => Ok(Some(info)),
            None if is_current_market => Ok(None),
            None => err!(CoreError::NotFound),
        }
    }
}

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
) -> Result<Vec<Pubkey>> {
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
        validated_market_tokens.push(meta.market_token_mint);
    }

    require_keys_eq!(current, *token_out, CoreError::InvalidSwapPath);

    Ok(validated_market_tokens)
}

impl From<SwapActionParamsError> for CoreError {
    fn from(err: SwapActionParamsError) -> Self {
        msg!("Swap params error: {}", err);
        match err {
            SwapActionParamsError::InvalidSwapPath(_) => Self::InvalidSwapPath,
        }
    }
}
