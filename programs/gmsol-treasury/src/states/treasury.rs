use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_store::{utils::pubkey::to_bytes, CoreError};

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

impl gmsol_utils::InitSpace for Treasury {
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
}

/// Token config for treasury.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenConfig {
    flags: TokenFlagContainer,
    reserved: [u8; 256],
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
