use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use gmsol_store::{
    states::{Oracle, Seed},
    utils::pubkey::to_bytes,
    CoreError,
};
use gmsol_utils::gt::{GtBankFlags, MAX_GT_BANK_FLAGS};

use super::treasury::MAX_TOKENS;

/// GT Bank.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct GtBank {
    version: u8,
    pub(crate) bump: u8,
    flags: GtBankFlagsContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 13],
    pub(crate) treasury_vault_config: Pubkey,
    pub(crate) gt_exchange_vault: Pubkey,
    remaining_confirmed_gt_amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
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
        treasury_vault_config: Pubkey,
        gt_exchange_vault: Pubkey,
    ) -> Result<()> {
        require!(
            !self.flags.get_flag(GtBankFlags::Initialized),
            CoreError::PreconditionsAreNotMet
        );
        self.bump = bump;
        self.treasury_vault_config = treasury_vault_config;
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

    /// Get balance of the given token
    pub fn get_balance(&self, token: &Pubkey) -> Option<u64> {
        self.balances.get(token).map(|b| b.amount)
    }

    /// Iterate over token balances.
    #[cfg(feature = "utils")]
    pub fn balances(&self) -> impl Iterator<Item = (Pubkey, u64)> + '_ {
        self.balances
            .entries()
            .map(|(k, b)| (Pubkey::new_from_array(*k), b.amount))
    }

    /// Get treasury vault config address.
    #[cfg(feature = "utils")]
    pub fn treasury_vault_config(&self) -> &Pubkey {
        &self.treasury_vault_config
    }

    /// Get GT exchange vault address.
    #[cfg(feature = "utils")]
    pub fn gt_exchange_vault(&self) -> &Pubkey {
        &self.gt_exchange_vault
    }

    /// Get the number of tokens.
    pub fn num_tokens(&self) -> usize {
        self.balances.len()
    }

    /// Get all tokens.
    pub fn tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.balances
            .entries()
            .map(|(key, _)| Pubkey::new_from_array(*key))
    }

    /// Create tokens with feed.
    #[cfg(feature = "utils")]
    pub fn to_feeds(
        &self,
        map: &impl gmsol_store::states::TokenMapAccess,
        treasury_vault_config: &super::TreasuryVaultConfig,
    ) -> Result<gmsol_store::states::common::TokensWithFeed> {
        use std::collections::BTreeSet;

        use gmsol_store::states::common::{TokenRecord, TokensWithFeed};

        let tokens = self
            .tokens()
            .chain(treasury_vault_config.tokens())
            .collect::<BTreeSet<_>>();
        let records = tokens
            .iter()
            .map(|token| {
                let config = map
                    .get(token)
                    .ok_or_else(|| error!(CoreError::UnknownToken))?;
                TokenRecord::from_config(*token, config)
                    .map_err(CoreError::from)
                    .map_err(|err| error!(err))
            })
            .collect::<Result<Vec<_>>>()?;

        TokensWithFeed::try_from_records(records)
            .map_err(CoreError::from)
            .map_err(|err| error!(err))
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
        balance.amount = next_balance;

        Ok(())
    }

    pub(crate) fn record_all_transferred_out(&mut self) -> Result<()> {
        for (_key, balance) in self.balances.entries_mut() {
            balance.amount = 0;
        }
        Ok(())
    }

    /// # CHECK
    /// Must be called only after `amount` has been withdrawn from the receiver vault.
    pub(crate) fn increase_receiver_vault_out_unchecked(
        &mut self,
        token: &Pubkey,
        amount: u64,
    ) -> Result<()> {
        let balance = self
            .balances
            .get_mut(token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        balance.receiver_vault_out = balance
            .receiver_vault_out
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        Ok(())
    }

    /// Get receiver vault out amount.
    #[cfg(feature = "utils")]
    pub fn receiver_vault_out(&self, token: &Pubkey) -> Option<u64> {
        Some(self.balances.get(token)?.receiver_vault_out)
    }

    /// Returns whether the GT bank is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(GtBankFlags::Initialized)
    }

    pub(crate) fn total_value(&self, oracle: &Oracle) -> Result<u128> {
        let mut total_value: u128 = 0;

        for (token, balance) in self.balances.entries() {
            let amount = u128::from(balance.amount);
            if amount == 0 {
                continue;
            }
            let token = Pubkey::new_from_array(*token);
            let price = oracle.get_primary_price(&token, false)?.min;
            let value = amount
                .checked_mul(price)
                .ok_or_else(|| error!(CoreError::ValueOverflow))?;

            if value != 0 {
                total_value = total_value
                    .checked_add(value)
                    .ok_or_else(|| error!(CoreError::ValueOverflow))?;
            }
        }

        Ok(total_value)
    }

    /// Reserve the balances of the given proportion.
    /// # Warning
    /// This method is not atomic.
    pub(crate) fn reserve_balances(&mut self, numerator: &u128, denominator: &u128) -> Result<()> {
        use gmsol_model::num::MulDiv;

        require_gte!(denominator, numerator, CoreError::InvalidArgument);

        for (_key, balance) in self.balances.entries_mut() {
            if balance.amount == 0 {
                continue;
            }
            let reserve_balance = u128::from(balance.amount)
                .checked_mul_div(numerator, denominator)
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            require_gte!(balance.amount, reserve_balance, CoreError::Internal);
            balance.amount = reserve_balance;
        }

        Ok(())
    }

    pub(crate) fn signer(&self) -> GtBankSigner {
        GtBankSigner {
            treasury_vault_config: self.treasury_vault_config,
            gt_exchange_vault: self.gt_exchange_vault,
            bump_bytes: [self.bump],
        }
    }

    /// # CHECK
    /// `gt_amount` must be the total amount of the GT exchange vault
    /// of this bank and it must have been confirmed.
    pub(crate) fn confirm_unchecked(&mut self, gt_amount: u64) -> Result<()> {
        let previsous = self.flags.set_flag(GtBankFlags::Confirmed, true);
        require_eq!(previsous, false, {
            msg!("[GT Bank] this GT bank has been confirmed");
            CoreError::PreconditionsAreNotMet
        });
        self.remaining_confirmed_gt_amount = gt_amount;
        Ok(())
    }

    /// Returns whether the GT bank is confirmed.
    pub fn is_confirmed(&self) -> bool {
        self.flags.get_flag(GtBankFlags::Confirmed)
    }

    pub(crate) fn record_claimed(&mut self, gt_amount: u64) -> Result<()> {
        let next_amount = self
            .remaining_confirmed_gt_amount
            .checked_sub(gt_amount)
            .ok_or_else(|| error!(CoreError::InvalidArgument))?;
        self.remaining_confirmed_gt_amount = next_amount;
        Ok(())
    }

    pub(crate) fn remaining_confirmed_gt_amount(&self) -> u64 {
        self.remaining_confirmed_gt_amount
    }

    /// # CHECK
    /// Only call this after syncing the GT bank.
    pub(crate) fn handle_synced_unchecked(&mut self) -> Result<()> {
        if self.is_confirmed() && !self.is_synced_after_confirmation() {
            self.flags
                .set_flag(GtBankFlags::SyncedAfterConfirmation, true);
        }
        Ok(())
    }

    /// Returns whether this GT bank has been synced after confirmation.
    pub fn is_synced_after_confirmation(&self) -> bool {
        self.flags.get_flag(GtBankFlags::SyncedAfterConfirmation)
    }
}

/// Gt Bank Signer.
pub struct GtBankSigner {
    treasury_vault_config: Pubkey,
    gt_exchange_vault: Pubkey,
    bump_bytes: [u8; 1],
}

impl GtBankSigner {
    /// As signer seeds.
    pub fn as_seeds(&self) -> [&[u8]; 4] {
        [
            GtBank::SEED,
            self.treasury_vault_config.as_ref(),
            self.gt_exchange_vault.as_ref(),
            &self.bump_bytes,
        ]
    }
}

/// Token Balance.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct TokenBalance {
    amount: u64,
    receiver_vault_out: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 56],
}

impl Default for TokenBalance {
    fn default() -> Self {
        Self::zeroed()
    }
}

gmsol_utils::fixed_map!(TokenBalances, Pubkey, to_bytes, TokenBalance, MAX_TOKENS, 4);

gmsol_utils::flags!(GtBankFlags, MAX_GT_BANK_FLAGS, u8);
