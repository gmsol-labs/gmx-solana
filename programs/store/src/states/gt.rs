//! # GT
//!
//! GT is designed to boost trading activity on GMX-Solana by offering an unique incentive mechanism
//! that provides traders with additional rewards to maximize rebates and minimize trading costs. GT
//! is a highly customizable token obtainable through trading on GMX-Solana. The initial mint cost
//! of GT is $0.05, equivalent to a trading volume of $100 with a 5 bps fee. As the total supply
//! grows, GT's mint cost will increase exponentially. Each cycle, set at 100,000 GT, raises the
//! mint cost by 1%.
//!
//! #### Treasury and Buyback
//!
//! GT is non-transferable, and its sale can only occur through the daily Treasury buyback. 60% of
//! the fees will go to the Treasury. Each day, the Treasury will allocate USDC for buybacks based
//! on the min {deposited GT * mint cost, 2% of the Treasury, 60% of the Treasury's daily revenue}.
//! The actual buyback price will be determined by dividing the total allocated USDC by the total
//! amount of GT deposited by users. After the buyback is complete, users can claim the
//! corresponding USDC.
//!
//! #### VIP Levels (User Ranks)
//!
//! VIP levels are assigned based on users' GT holdings. The more GT a user holds, the higher their
//! VIP level, which grants greater order fee discounts.
//!
//! #### Referral Program
//!
//! The referral program offers referees an extra 10% order fee discount. The final order fee discount
//! will be calculated as: order fee discount = 1 - (1 - order fee vip discount) * (1 - order fee
//! referred discount).

use anchor_lang::prelude::*;
use gmsol_utils::gt::{
    GtExchangeFlag, GtExchangeVaultFlag, MAX_GT_EXCHANGE_FLAGS, MAX_GT_EXCHANGE_VAULT_FLAGS,
};

use crate::{constants, CoreError};

use super::{user::UserHeader, Seed};

pub use gmsol_utils::gt::get_time_window_index;

const MAX_RANK: usize = 15;

#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct GtState {
    decimals: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 7],
    /* States */
    pub(crate) last_minted_at: i64,
    total_minted: u64,
    /// Grow step amount. It must be immutable.
    grow_step_amount: u64,
    grow_steps: u64,
    /// Supply of buybackable GT.
    supply: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 8],
    /// Vault for non-buybackable GT.
    gt_vault: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_2: [u8; 16],
    /* Configs */
    minting_cost_grow_factor: u128,
    minting_cost: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_3: [u8; 32],
    exchange_time_window: u32,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_4: [u8; 12],
    max_rank: u64,
    ranks: [u64; MAX_RANK],
    order_fee_discount_factors: [u128; MAX_RANK + 1],
    referral_reward_factors: [u128; MAX_RANK + 1],
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_5: [u8; 32],
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 256],
}

impl GtState {
    pub(crate) fn init(
        &mut self,
        decimals: u8,
        initial_minting_cost: u128,
        grow_factor: u128,
        grow_step: u64,
        ranks: &[u64],
    ) -> Result<()> {
        require!(!self.is_initialized(), CoreError::GTStateHasBeenInitialized);
        require_eq!(self.last_minted_at, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.total_minted, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.supply, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.gt_vault, 0, CoreError::GTStateHasBeenInitialized);

        require!(grow_step != 0, CoreError::InvalidGTConfig);

        let max_rank = ranks.len().min(MAX_RANK);
        let ranks = &ranks[0..max_rank];

        // Ranks must be storted.
        require!(
            ranks.windows(2).all(|ab| {
                if let [a, b] = &ab {
                    a < b
                } else {
                    false
                }
            }),
            CoreError::InvalidGTConfig
        );

        let clock = Clock::get()?;

        self.decimals = decimals;
        self.last_minted_at = clock.unix_timestamp;
        self.grow_step_amount = grow_step;
        self.minting_cost_grow_factor = grow_factor;
        self.minting_cost = initial_minting_cost;

        let target = &mut self.ranks[0..max_rank];
        target.copy_from_slice(ranks);
        self.max_rank = max_rank as u64;

        self.exchange_time_window = constants::DEFAULT_GT_VAULT_TIME_WINDOW;

        Ok(())
    }

    /// Returns whether the GT state is initialized.
    pub fn is_initialized(&self) -> bool {
        self.grow_step_amount != 0
    }

    pub(crate) fn set_order_fee_discount_factors(&mut self, factors: &[u128]) -> Result<()> {
        require_eq!(
            factors.len(),
            (self.max_rank + 1) as usize,
            CoreError::InvalidArgument
        );

        require!(
            factors
                .iter()
                .all(|factor| *factor <= constants::MARKET_USD_UNIT),
            CoreError::InvalidArgument
        );

        let target = &mut self.order_fee_discount_factors[0..factors.len()];
        target.copy_from_slice(factors);

        Ok(())
    }

    pub(crate) fn set_referral_reward_factors(&mut self, factors: &[u128]) -> Result<()> {
        require_eq!(
            factors.len(),
            (self.max_rank + 1) as usize,
            CoreError::InvalidArgument
        );

        // Factors must be storted.
        require!(
            factors.windows(2).all(|ab| {
                if let [a, b] = &ab {
                    a <= b
                } else {
                    false
                }
            }),
            CoreError::InvalidArgument
        );

        let target = &mut self.referral_reward_factors[0..factors.len()];
        target.copy_from_slice(factors);

        Ok(())
    }

    pub(crate) fn order_fee_discount_factor(&self, rank: u8) -> Result<u128> {
        require_gte!(self.max_rank, rank as u64, CoreError::InvalidArgument);
        Ok(self.order_fee_discount_factors[rank as usize])
    }

    pub(crate) fn referral_reward_factor(&self, rank: u8) -> Result<u128> {
        require_gte!(self.max_rank, rank as u64, CoreError::InvalidArgument);
        Ok(self.referral_reward_factors[rank as usize])
    }

    /// Get time window for GT exchange.
    pub fn exchange_time_window(&self) -> u32 {
        self.exchange_time_window
    }

    /// Get GT decimals.
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Get minting cost.
    pub fn minting_cost(&self) -> u128 {
        self.minting_cost
    }

    /// Get total minted.
    pub fn total_minted(&self) -> u64 {
        self.total_minted
    }

    /// Get grow steps.
    pub fn grow_steps(&self) -> u64 {
        self.grow_steps
    }

    /// Get GT supply.
    pub fn supply(&self) -> u64 {
        self.supply
    }

    /// Get GT vault.
    pub fn gt_vault(&self) -> u64 {
        self.gt_vault
    }

    /// Set exchange time window.
    pub fn set_exchange_time_window(&mut self, window: u32) -> Result<()> {
        require_neq!(window, 0, CoreError::InvalidArgument);
        self.exchange_time_window = window;
        Ok(())
    }

    fn next_minting_cost(&self, next_minted: u64) -> Result<Option<(u64, u128)>> {
        use gmsol_model::utils::apply_factor;

        require!(self.grow_step_amount != 0, CoreError::InvalidGTConfig);
        let new_steps = next_minted / self.grow_step_amount;

        if new_steps != self.grow_steps {
            let mut minting_cost = self.minting_cost;
            for _ in self.grow_steps..new_steps {
                minting_cost = apply_factor::<_, { constants::MARKET_DECIMALS }>(
                    &minting_cost,
                    &self.minting_cost_grow_factor,
                )
                .ok_or_else(|| error!(CoreError::Internal))?;
            }
            Ok(Some((new_steps, minting_cost)))
        } else {
            Ok(None)
        }
    }

    /// CHECK: the user must be owned by this store.
    fn unchecked_update_rank(&self, user: &mut UserHeader) {
        debug_assert!(self.ranks().len() < u8::MAX as usize);
        let rank = match self.ranks().binary_search(&user.gt.amount) {
            Ok(rank) => rank + 1,
            Err(rank) => rank,
        };

        let rank = rank as u8;
        if user.gt.rank != rank {
            user.gt.rank = rank;
            msg!("[GT] user rank updated, new rank = {}", rank);
        }
    }

    #[inline(never)]
    pub(crate) fn mint_to(&mut self, user: &mut UserHeader, amount: u64) -> Result<()> {
        if amount != 0 {
            let clock = Clock::get()?;

            // Calculate global GT state updates.
            let next_gt_total_minted = self
                .total_minted
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            let next_minting_cost = self.next_minting_cost(next_gt_total_minted)?;

            // Calculate user GT state updates.
            let next_user_total_minted = user
                .gt
                .total_minted
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            let next_amount = user
                .gt
                .amount
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            let next_supply = self
                .supply
                .checked_add(amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

            /* The following steps should be infallible. */

            if let Some((new_steps, new_minting_cost)) = next_minting_cost {
                self.minting_cost = new_minting_cost;
                self.grow_steps = new_steps;
            }
            self.total_minted = next_gt_total_minted;
            self.last_minted_at = clock.unix_timestamp;

            user.gt.total_minted = next_user_total_minted;
            user.gt.amount = next_amount;
            user.gt.last_minted_at = self.last_minted_at;
            self.supply = next_supply;

            self.unchecked_update_rank(user);
        }
        Ok(())
    }

    /// Burn GT from the given `user`.
    ///
    /// # CHECK
    /// - The `user` must be owned by this store.
    ///
    /// # Errors
    /// - `user` must have enough amount of GT.
    pub(crate) fn unchecked_burn_from(&mut self, user: &mut UserHeader, amount: u64) -> Result<()> {
        if amount != 0 {
            require_gte!(user.gt.amount, amount, CoreError::NotEnoughTokenAmount);
            let next_amount = user
                .gt
                .amount
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::Internal))?;

            let next_supply = self
                .supply
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::Internal))?;

            /* The following steps should be infallible. */

            user.gt.amount = next_amount;
            self.supply = next_supply;

            self.unchecked_update_rank(user);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn get_mint_amount(&self, size_in_value: u128) -> Result<(u64, u128, u128)> {
        let minting_cost = self.minting_cost;

        require!(minting_cost != 0, CoreError::InvalidGTConfig);

        let remainder = size_in_value % minting_cost;
        let minted = (size_in_value / minting_cost)
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

        let minted_value = size_in_value - remainder;

        Ok((minted, minted_value, minting_cost))
    }

    pub(crate) fn ranks(&self) -> &[u64] {
        &self.ranks[0..(self.max_rank as usize)]
    }

    /// Request an exchange.
    ///
    /// # CHECK
    /// - `user`, `vault` and `exchange` must owned by this store.
    ///
    /// # Errors
    /// - `user`, `vault` and `exchange` must have been initialized.
    /// - `vault` must be depositable.
    /// - `user` must have enough amount of GT.
    ///
    /// # Notes
    /// - This is not an atomic operation.
    pub(crate) fn unchecked_request_exchange(
        &mut self,
        user: &mut UserHeader,
        vault: &mut GtExchangeVault,
        exchange: &mut GtExchange,
        amount: u64,
    ) -> Result<()> {
        require!(user.is_initialized(), CoreError::InvalidArgument);
        require!(vault.is_initialized(), CoreError::InvalidArgument);
        require!(exchange.is_initialized(), CoreError::InvalidArgument);

        self.unchecked_burn_from(user, amount)?;

        vault.add(amount)?;
        exchange.add(amount)?;

        Ok(())
    }

    /// Confirm the exchange vault.
    ///
    /// # CHECK
    /// - `vault` must be owned by this store.
    ///
    /// # Errors
    /// - `vault` must have been initialized.
    /// - `vault` must be confirmable.
    pub(crate) fn unchecked_confirm_exchange_vault(
        &mut self,
        vault: &mut GtExchangeVault,
    ) -> Result<u64> {
        require!(vault.is_initialized(), CoreError::InvalidArgument);

        let amount = vault.confirm()?;

        self.process_gt_vault(amount)?;

        Ok(amount)
    }

    fn process_gt_vault(&mut self, amount: u64) -> Result<()> {
        if amount != 0 {
            let amount_for_vault = amount;

            let next_gt_vault = self
                .gt_vault
                .checked_add(amount_for_vault)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

            self.gt_vault = next_gt_vault;
        }
        Ok(())
    }
}

gmsol_utils::flags!(GtExchangeVaultFlag, MAX_GT_EXCHANGE_VAULT_FLAGS, u8);

/// GT Exchange Vault.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GtExchangeVault {
    /// Bump seed.
    pub bump: u8,
    flags: GtExchangeVaultFlagContainer,
    padding: [u8; 6],
    ts: i64,
    time_window: i64,
    amount: u64,
    /// Store.
    pub store: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl GtExchangeVault {
    /// Get amount.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Get whether the vault is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(GtExchangeVaultFlag::Intiailized)
    }

    /// Get whether the vault is comfirmed.
    pub fn is_confirmed(&self) -> bool {
        self.flags.get_flag(GtExchangeVaultFlag::Comfirmed)
    }

    pub(crate) fn init(&mut self, bump: u8, store: &Pubkey, time_window: u32) -> Result<()> {
        require!(!self.is_initialized(), CoreError::PreconditionsAreNotMet);

        require!(time_window != 0, CoreError::InvalidArgument);

        let clock = Clock::get()?;

        self.bump = bump;
        self.ts = clock.unix_timestamp;
        self.store = *store;
        self.flags.set_flag(GtExchangeVaultFlag::Intiailized, true);
        self.time_window = i64::from(time_window);

        Ok(())
    }

    /// Get current time window index.
    pub fn time_window_index(&self) -> i64 {
        get_time_window_index(self.ts, self.time_window)
    }

    /// Get time window.
    pub fn time_window(&self) -> i64 {
        self.time_window
    }

    /// Get time window as `u32`.
    pub fn time_window_u32(&self) -> u32 {
        self.time_window.try_into().expect("invalid vault")
    }

    /// Validate that this vault is confirmable.
    pub fn validate_confirmable(&self) -> Result<()> {
        require!(self.is_initialized(), CoreError::PreconditionsAreNotMet);
        require!(!self.is_confirmed(), CoreError::PreconditionsAreNotMet);

        let clock = Clock::get()?;
        let current_index = get_time_window_index(clock.unix_timestamp, self.time_window);

        require_gt!(
            current_index,
            self.time_window_index(),
            CoreError::PreconditionsAreNotMet
        );

        Ok(())
    }

    /// Confirm the vault.
    fn confirm(&mut self) -> Result<u64> {
        self.validate_confirmable()?;
        self.flags.set_flag(GtExchangeVaultFlag::Comfirmed, true);
        Ok(self.amount)
    }

    /// Validate that this vault is depositable.
    pub fn validate_depositable(&self) -> Result<()> {
        require!(!self.is_confirmed(), CoreError::PreconditionsAreNotMet);

        let clock = Clock::get()?;
        let current_index = get_time_window_index(clock.unix_timestamp, self.time_window);
        require_eq!(
            current_index,
            self.time_window_index(),
            CoreError::InvalidArgument
        );
        Ok(())
    }

    /// Add GT to this vault.
    ///
    /// # Errors
    /// - This vault must be depositable.
    /// - Error on amount overflow.
    fn add(&mut self, amount: u64) -> Result<()> {
        self.validate_depositable()?;

        self.amount = self
            .amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        Ok(())
    }
}

impl Seed for GtExchangeVault {
    const SEED: &'static [u8] = b"gt_exchange_vault";
}

impl gmsol_utils::InitSpace for GtExchangeVault {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

gmsol_utils::flags!(GtExchangeFlag, MAX_GT_EXCHANGE_FLAGS, u8);

/// GT Exchange Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GtExchange {
    /// Bump.
    pub bump: u8,
    flags: GtExchangeFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 6],
    amount: u64,
    /// Owner address.
    pub owner: Pubkey,
    /// Store address.
    pub store: Pubkey,
    /// Vault address.
    pub vault: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl Default for GtExchange {
    fn default() -> Self {
        use bytemuck::Zeroable;

        Self::zeroed()
    }
}

impl GtExchange {
    /// Get whether the vault is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(GtExchangeFlag::Intiailized)
    }

    pub(crate) fn init(
        &mut self,
        bump: u8,
        owner: &Pubkey,
        store: &Pubkey,
        vault: &Pubkey,
    ) -> Result<()> {
        require!(!self.is_initialized(), CoreError::PreconditionsAreNotMet);

        self.bump = bump;
        self.owner = *owner;
        self.store = *store;
        self.vault = *vault;

        self.flags.set_flag(GtExchangeFlag::Intiailized, true);

        Ok(())
    }

    /// Add GT amount.
    fn add(&mut self, amount: u64) -> Result<()> {
        self.amount = self
            .amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        Ok(())
    }

    /// Get the owner address.
    pub fn owner(&self) -> &Pubkey {
        &self.owner
    }

    pub(crate) fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Get vault.
    pub fn vault(&self) -> &Pubkey {
        &self.vault
    }

    /// Get amount.
    pub fn amount(&self) -> u64 {
        self.amount
    }
}

impl gmsol_utils::InitSpace for GtExchange {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for GtExchange {
    const SEED: &'static [u8] = b"gt_exchange";
}
