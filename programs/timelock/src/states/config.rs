use anchor_lang::prelude::*;
use gmsol_store::{states::Seed, CoreError};

/// Timelock Config.
#[account(zero_copy)]
pub struct TimelockConfig {
    version: u8,
    pub(crate) bump: u8,
    padding_0: [u8; 6],
    delay: u32,
    padding_1: [u8; 4],
    pub(crate) store: Pubkey,
    reserved: [u8; 256],
}

impl Seed for TimelockConfig {
    const SEED: &'static [u8] = b"timelock_config";
}

impl gmsol_utils::InitSpace for TimelockConfig {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl TimelockConfig {
    pub(crate) fn init(&mut self, bump: u8, delay: u32, store: Pubkey) {
        self.bump = bump;
        self.delay = delay;
        self.store = store;
    }

    /// Get delay.
    pub fn delay(&self) -> u32 {
        self.delay
    }

    /// Increase delay.
    pub(crate) fn increase_delay(&mut self, delta: u32) -> Result<u32> {
        let new_delay = self
            .delay
            .checked_add(delta)
            .ok_or_else(|| error!(CoreError::InvalidArgument))?;
        self.delay = new_delay;
        Ok(new_delay)
    }
}
