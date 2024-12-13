use anchor_lang::prelude::*;
use gmsol_store::{states::Seed, CoreError};
use gmsol_utils::InitSpace;

/// Config account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Config {
    pub(crate) bump: u8,
    padding: [u8; 7],
    pub(crate) store: Pubkey,
    treasury: Pubkey,
    reserved: [u8; 256],
}

impl Seed for Config {
    const SEED: &'static [u8] = b"config";
}

impl InitSpace for Config {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Config {
    pub(crate) fn init(&mut self, bump: u8, store: &Pubkey) {
        self.bump = bump;
        self.store = *store;
    }

    /// Get the treasury address.
    pub fn treasury(&self) -> Option<&Pubkey> {
        if self.treasury == Pubkey::default() {
            None
        } else {
            Some(&self.treasury)
        }
    }

    /// Set the treasury address.
    pub(crate) fn set_treasury(&mut self, mut address: Pubkey) -> Result<Pubkey> {
        require_neq!(self.treasury, address, CoreError::PreconditionsAreNotMet);

        std::mem::swap(&mut address, &mut self.treasury);

        Ok(address)
    }
}
