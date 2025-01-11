use anchor_lang::prelude::*;
use gmsol_store::{states::Seed, utils::pubkey::optional_address, CoreError};
use gmsol_utils::InitSpace;

use crate::constants;

/// Global Config account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Config {
    pub(crate) bump: u8,
    padding_0: [u8; 15],
    pub(crate) store: Pubkey,
    treasury_config: Pubkey,
    gt_factor: u128,
    buyback_factor: u128,
    padding_1: [u8; 48],
    padding_2: [u8; 64],
    reserved: [u8; 128],
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

    /// Get the treasury config address.
    pub fn treasury_config(&self) -> Option<&Pubkey> {
        optional_address(&self.treasury_config)
    }

    /// Set the treasury config address.
    pub(crate) fn set_treasury_config(&mut self, mut address: Pubkey) -> Result<Pubkey> {
        require_neq!(
            self.treasury_config,
            address,
            CoreError::PreconditionsAreNotMet
        );

        std::mem::swap(&mut address, &mut self.treasury_config);

        Ok(address)
    }

    /// Get GT factor.
    pub fn gt_factor(&self) -> u128 {
        self.gt_factor
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

    /// Get buyback factor.
    pub fn buyback_factor(&self) -> u128 {
        self.buyback_factor
    }

    /// Set buyback factor.
    pub(crate) fn set_buyback_factor(&mut self, mut factor: u128) -> Result<u128> {
        require_gte!(
            gmsol_store::constants::MARKET_USD_UNIT,
            factor,
            CoreError::InvalidArgument
        );
        require_neq!(
            self.buyback_factor,
            factor,
            CoreError::PreconditionsAreNotMet
        );
        std::mem::swap(&mut self.buyback_factor, &mut factor);
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

/// Receiver Signer.
pub struct ReceiverSigner {
    config: Pubkey,
    bump_bytes: [u8; 1],
}

impl ReceiverSigner {
    /// Create from config address and bump.
    pub fn new(config: Pubkey, bump: u8) -> Self {
        Self {
            config,
            bump_bytes: [bump],
        }
    }

    /// As signer seeds.
    pub fn as_seeds(&self) -> [&[u8]; 3] {
        [
            constants::RECEIVER_SEED,
            self.config.as_ref(),
            &self.bump_bytes,
        ]
    }
}
