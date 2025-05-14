use std::collections::BTreeSet;

use anchor_lang::prelude::{
    borsh::{BorshDeserialize, BorshSerialize},
    *,
};

/// Market error.
#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    /// Not a collateral token.
    #[error("not a collateral token")]
    NotACollateralToken,
}

type MarketResult<T> = std::result::Result<T, MarketError>;

/// Market Metadata.
#[zero_copy]
#[derive(BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MarketMeta {
    /// Market token.
    pub market_token_mint: Pubkey,
    /// Index token.
    pub index_token_mint: Pubkey,
    /// Long token.
    pub long_token_mint: Pubkey,
    /// Short token.
    pub short_token_mint: Pubkey,
}

impl MarketMeta {
    /// Check if the given token is a valid collateral token.
    #[inline]
    pub fn is_collateral_token(&self, token: &Pubkey) -> bool {
        *token == self.long_token_mint || *token == self.short_token_mint
    }

    /// Get pnl token.
    pub fn pnl_token(&self, is_long: bool) -> Pubkey {
        if is_long {
            self.long_token_mint
        } else {
            self.short_token_mint
        }
    }

    /// Check if the given token is long token or short token, and return it's side.
    pub fn to_token_side(&self, token: &Pubkey) -> MarketResult<bool> {
        if *token == self.long_token_mint {
            Ok(true)
        } else if *token == self.short_token_mint {
            Ok(false)
        } else {
            Err(MarketError::NotACollateralToken)
        }
    }

    /// Get opposite token.
    pub fn opposite_token(&self, token: &Pubkey) -> MarketResult<&Pubkey> {
        if *token == self.long_token_mint {
            Ok(&self.short_token_mint)
        } else if *token == self.short_token_mint {
            Ok(&self.long_token_mint)
        } else {
            Err(MarketError::NotACollateralToken)
        }
    }

    /// Get ordered token set.
    pub fn ordered_tokens(&self) -> BTreeSet<Pubkey> {
        BTreeSet::from([
            self.index_token_mint,
            self.long_token_mint,
            self.short_token_mint,
        ])
    }
}

/// Type that has market meta.
pub trait HasMarketMeta {
    fn market_meta(&self) -> &MarketMeta;

    fn is_pure(&self) -> bool {
        let meta = self.market_meta();
        meta.long_token_mint == meta.short_token_mint
    }
}

impl HasMarketMeta for MarketMeta {
    fn market_meta(&self) -> &MarketMeta {
        self
    }
}

/// Get related tokens from markets in order.
pub fn ordered_tokens(from: &impl HasMarketMeta, to: &impl HasMarketMeta) -> BTreeSet<Pubkey> {
    let mut tokens = BTreeSet::default();

    let from = from.market_meta();
    let to = to.market_meta();

    for mint in [
        &from.index_token_mint,
        &from.long_token_mint,
        &from.short_token_mint,
    ]
    .iter()
    .chain(&[
        &to.index_token_mint,
        &to.long_token_mint,
        &to.short_token_mint,
    ]) {
        tokens.insert(**mint);
    }
    tokens
}
