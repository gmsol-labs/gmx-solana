use anchor_lang::prelude::*;
use gmsol_store::{states::Seed, CoreError};
use gmsol_utils::InitSpace;

/// Config account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Config {
    pub(crate) bump: u8,
    padding: [u8; 15],
    pub(crate) store: Pubkey,
    treasury: Pubkey,
    gt_factor: u128,
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

    /// Set GT factor.
    pub(crate) fn set_gt_factor(&mut self, mut factor: u128) -> Result<u128> {
        require_gte!(
            gmsol_store::constants::MARKET_USD_UNIT,
            factor,
            CoreError::InvalidArgument
        );
        require_neq!(self.gt_factor, factor, CoreError::PreconditionsAreNotMet);
        std::mem::swap(&mut self.gt_factor, &mut factor);
        Ok(factor)
    }

    /// Get signer.
    pub(crate) fn signer(&self) -> ConfigSigner {
        ConfigSigner {
            store: self.store,
            bump_bytes: [self.bump],
        }
    }
}

/// Config Signer.
pub struct ConfigSigner {
    store: Pubkey,
    bump_bytes: [u8; 1],
}

impl ConfigSigner {
    /// As signer seeds.
    pub fn as_seeds(&self) -> [&[u8]; 3] {
        [Config::SEED, self.store.as_ref(), &self.bump_bytes]
    }
}
