use anchor_lang::prelude::*;
use gmsol_store::{
    states::{Seed, MAX_ROLE_NAME_LEN},
    utils::fixed_str::{bytes_to_fixed_str, fixed_str_to_bytes},
};

/// Executor.
#[account(zero_copy)]
pub struct Executor {
    pub(crate) bump: u8,
    padding: [u8; 15],
    pub(crate) store: Pubkey,
    role_name: [u8; MAX_ROLE_NAME_LEN],
    reserved: [u8; 256],
}

impl Executor {
    /// Get role name.
    pub fn role_name(&self) -> Result<&str> {
        bytes_to_fixed_str(&self.role_name)
    }

    pub(crate) fn try_init(&mut self, bump: u8, store: Pubkey, role_name: &str) -> Result<()> {
        let role_name = fixed_str_to_bytes(role_name)?;
        self.bump = bump;
        self.store = store;
        self.role_name = role_name;
        Ok(())
    }

    pub(crate) fn signer(&self) -> ExecutorSigner {
        ExecutorSigner {
            store: self.store,
            role_name: self.role_name,
            bump_bytes: [self.bump],
        }
    }
}

impl Seed for Executor {
    const SEED: &'static [u8] = b"timelock_executor";
}

impl gmsol_utils::InitSpace for Executor {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

/// Executor Signer.
pub struct ExecutorSigner {
    store: Pubkey,
    role_name: [u8; MAX_ROLE_NAME_LEN],
    bump_bytes: [u8; 1],
}

impl ExecutorSigner {
    pub(crate) fn as_seeds(&self) -> [&[u8]; 4] {
        [
            Executor::SEED,
            self.store.as_ref(),
            self.role_name.as_ref(),
            &self.bump_bytes,
        ]
    }
}
