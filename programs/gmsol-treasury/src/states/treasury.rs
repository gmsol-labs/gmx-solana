use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_store::{states::Seed, utils::pubkey::to_bytes, CoreError};

pub(crate) const MAX_TOKENS: usize = 16;

/// Treasury vault config account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct TreasuryVaultConfig {
    version: u8,
    pub(crate) bump: u8,
    index: u16,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 12],
    pub(crate) config: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 256],
    tokens: TokenMap,
}

impl Seed for TreasuryVaultConfig {
    const SEED: &'static [u8] = b"treasury_vault_config";
}

impl gmsol_utils::InitSpace for TreasuryVaultConfig {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl TreasuryVaultConfig {
    pub(crate) fn init(&mut self, bump: u8, index: u16, config: &Pubkey) {
        self.bump = bump;
        self.index = index;
        self.config = *config;
    }

    pub(crate) fn insert_token(&mut self, token: &Pubkey) -> Result<()> {
        self.tokens
            .insert_with_options(token, TokenConfig::default(), true)?;
        Ok(())
    }

    pub(crate) fn remove_token(&mut self, token: &Pubkey) -> Result<()> {
        self.tokens
            .remove(token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        Ok(())
    }

    pub(crate) fn toggle_token_flag(
        &mut self,
        token: &Pubkey,
        flag: TokenFlag,
        value: bool,
    ) -> Result<bool> {
        let config = self
            .tokens
            .get_mut(token)
            .ok_or_else(|| error!(CoreError::NotFound))?;

        require_neq!(
            config.flags.get_flag(flag),
            value,
            CoreError::PreconditionsAreNotMet
        );

        Ok(config.flags.set_flag(flag, value))
    }

    fn get_token_config(&self, token: &Pubkey) -> Result<&TokenConfig> {
        self.tokens
            .get(token)
            .ok_or_else(|| error!(CoreError::NotFound))
    }

    pub(crate) fn is_deposit_allowed(&self, token: &Pubkey) -> Result<bool> {
        Ok(self
            .get_token_config(token)?
            .flags
            .get_flag(TokenFlag::AllowDeposit))
    }

    pub(crate) fn is_withdrawal_allowed(&self, token: &Pubkey) -> Result<bool> {
        Ok(self
            .get_token_config(token)?
            .flags
            .get_flag(TokenFlag::AllowWithdrawal))
    }

    pub(crate) fn signer(&self) -> TreasuryVaultSigner {
        TreasuryVaultSigner {
            config: self.config,
            index_bytes: self.index.to_le_bytes(),
            bump_bytes: [self.bump],
        }
    }

    /// Get the number of tokens.
    pub fn num_tokens(&self) -> usize {
        self.tokens.len()
    }

    /// Get all tokens.
    pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.tokens
            .entries()
            .map(|(key, _)| Pubkey::new_from_array(*key))
    }
}

/// Treasury Vault Signer.
pub struct TreasuryVaultSigner {
    config: Pubkey,
    index_bytes: [u8; 2],
    bump_bytes: [u8; 1],
}

impl TreasuryVaultSigner {
    /// As signer seeds.
    pub fn as_seeds(&self) -> [&[u8]; 4] {
        [
            TreasuryVaultConfig::SEED,
            self.config.as_ref(),
            &self.index_bytes,
            &self.bump_bytes,
        ]
    }
}

/// Token config for treasury.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenConfig {
    flags: TokenFlagContainer,
    reserved: [u8; 64],
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self::zeroed()
    }
}

gmsol_utils::fixed_map!(TokenMap, Pubkey, to_bytes, TokenConfig, MAX_TOKENS, 0);

const MAX_FLAGS: usize = 8;

/// Token Flags.
#[derive(
    num_enum::IntoPrimitive, Clone, Copy, strum::EnumString, strum::Display, PartialEq, Eq,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[repr(u8)]
pub enum TokenFlag {
    /// Allow deposit.
    AllowDeposit,
    /// Allow withdrawal.
    AllowWithdrawal,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(TokenFlag, MAX_FLAGS, u8);
