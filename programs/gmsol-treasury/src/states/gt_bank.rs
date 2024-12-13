use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_store::{states::Seed, utils::pubkey::to_bytes, CoreError};

use super::treasury::MAX_TOKENS;

/// GT Bank.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtBank {
    pub(crate) bump: u8,
    flags: GtBankFlagsContainer,
    padding: [u8; 14],
    pub(crate) config: Pubkey,
    pub(crate) gt_exchange_vault: Pubkey,
    reserved: [u8; 256],
    balances: TokenBalances,
}

impl Seed for GtBank {
    const SEED: &'static [u8] = b"gt_bank";
}

impl gmsol_utils::InitSpace for GtBank {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl GtBank {
    pub(crate) fn try_init(
        &mut self,
        bump: u8,
        gt_exchange_vault: Pubkey,
        config: Pubkey,
    ) -> Result<()> {
        require!(
            !self.flags.get_flag(GtBankFlags::Initialized),
            CoreError::PreconditionsAreNotMet
        );
        self.bump = bump;
        self.config = config;
        self.gt_exchange_vault = gt_exchange_vault;
        self.flags.set_flag(GtBankFlags::Initialized, true);
        Ok(())
    }

    fn get_balance_or_insert(&mut self, token: &Pubkey) -> Result<&mut TokenBalance> {
        if self.balances.get(token).is_none() {
            self.balances
                .insert_with_options(token, TokenBalance::default(), true)?;
        }
        self.get_balance_mut(token)
    }

    fn get_balance_mut(&mut self, token: &Pubkey) -> Result<&mut TokenBalance> {
        self.balances
            .get_mut(token)
            .ok_or_else(|| error!(CoreError::NotFound))
    }

    pub(crate) fn record_transferred_in(&mut self, token: &Pubkey, amount: u64) -> Result<()> {
        let balance = self.get_balance_or_insert(token)?;
        let next_balance = balance
            .amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        balance.amount = next_balance;
        Ok(())
    }

    pub(crate) fn record_transferred_out(&mut self, token: &Pubkey, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }

        let balance = self.get_balance_mut(token)?;
        let next_balance = balance
            .amount
            .checked_sub(amount)
            .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;

        if next_balance == 0 {
            self.balances.remove(token);
        } else {
            balance.amount = next_balance;
        }

        Ok(())
    }

    /// Returns whether the GT bank is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(GtBankFlags::Initialized)
    }
}

/// Token Balance.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct TokenBalance {
    amount: u64,
    reserved: [u8; 64],
}

impl Default for TokenBalance {
    fn default() -> Self {
        Self::zeroed()
    }
}

gmsol_utils::fixed_map!(TokenBalances, Pubkey, to_bytes, TokenBalance, MAX_TOKENS, 4);

const MAX_FLAGS: usize = 8;

/// Flags of GT Bank.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum GtBankFlags {
    /// Initialized.
    Initialized,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(GtBankFlags, MAX_FLAGS, u8);
