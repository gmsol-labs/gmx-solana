use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_store::utils::pubkey::to_bytes;
use gmsol_utils::{fixed_map, InitSpace};

const MAX_TOKENS: usize = 64;

/// Treasury account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Treasury {
    reserved_0: [u8; 8],
    pub(crate) config: Pubkey,
    reserved_1: [u8; 256],
    tokens: TokenMap,
}

impl InitSpace for Treasury {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Treasury {
    pub(crate) fn init(&mut self, config: &Pubkey) {
        self.config = *config;
    }

    pub(crate) fn insert_token(&mut self, token: &Pubkey) -> Result<()> {
        self.tokens
            .insert_with_options(token, TokenConfig::default(), true)?;
        Ok(())
    }
}

/// Token config for treasury.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenConfig {
    flags: Flags,
    reserved: [u8; 256],
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self::zeroed()
    }
}

fixed_map!(TokenMap, Pubkey, to_bytes, TokenConfig, MAX_TOKENS, 0);

#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Flags {
    value: u8,
}
