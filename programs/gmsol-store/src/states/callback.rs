use anchor_lang::prelude::*;
use gmsol_callback::CALLBACK_AUTHORITY_SEED;

/// Callback authority.
#[account]
#[derive(InitSpace)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct CallbackAuthority {
    /// Bump bytes.
    pub bump_bytes: [u8; 1],
}

impl CallbackAuthority {
    pub(crate) fn signer_seeds(&self) -> [&[u8]; 2] {
        [CALLBACK_AUTHORITY_SEED, &self.bump_bytes]
    }

    pub(crate) fn bump(&self) -> u8 {
        self.bump_bytes[0]
    }
}
