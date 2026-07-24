use anchor_lang::prelude::*;

use super::Seed;

/// Per-token access control account for builder fees.
///
/// One account exists per `(store, token_mint)` pair, and its existence is
/// required before a builder fee denominated in that token can be set on an
/// order.
///
/// This account is currently a placeholder and carries no behavior: it is
/// intended to support future access control on builder fee withdrawal
/// (e.g. a withdrawal timelock).
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuilderFeeTokenController {
    version: u8,
    /// Bump seed.
    pub(crate) bump: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 14],
    /// The store.
    pub(crate) store: Pubkey,
    /// The token mint this controller is for.
    pub(crate) token_mint: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 176],
}

static_assertions::const_assert_eq!(std::mem::size_of::<BuilderFeeTokenController>(), 256);

impl BuilderFeeTokenController {
    pub(crate) fn init(&mut self, store: &Pubkey, token_mint: &Pubkey, bump: u8) {
        self.bump = bump;
        self.store = *store;
        self.token_mint = *token_mint;
    }

    /// Get the store.
    pub fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Get the token mint this controller is for.
    pub fn token_mint(&self) -> &Pubkey {
        &self.token_mint
    }
}

impl Seed for BuilderFeeTokenController {
    const SEED: &'static [u8] = b"builder_fee_token_controller";
}

impl gmsol_utils::InitSpace for BuilderFeeTokenController {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}
