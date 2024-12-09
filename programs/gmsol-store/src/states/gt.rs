//! # GT
//!
//! GT is designed to boost trading activity on GMX-Solana by offering a unique incentive mechanism
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
//! #### esGT
//!
//! To encourage long-term holding of GT, esGT (also known as "staking rewards") has been
//! introduced. Each day, the amount of GT sold in the market generates an equivalent amount of esGT
//! on a 1:1 basis. Of this, 60% is distributed proportionally to users based on their GT holdings.
//!
//! #### Vesting
//!
//! Users can convert their esGT to GT through a vesting process. To initiate vesting, an amount of
//! GT equal to 5 times the esGT being vested must be locked. This locked GT cannot be sold during
//! the vesting period. esGT will be linearly converted to GT over a one-year period, with the
//! corresponding locked GT gradually unlocking as well. The converted GT retains the same
//! properties as GT minted from trading.
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

use crate::{constants, CoreError};

use super::{user::UserHeader, Seed};

#[cfg(feature = "utils")]
use std::num::NonZeroU64;

const MAX_RANK: usize = 15;
const MAX_FLAGS: usize = 8;

#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtState {
    decimals: u8,
    padding_0: [u8; 7],
    /* States */
    pub(crate) last_minted_at: i64,
    total_minted: u64,
    grow_step_amount: u64,
    grow_steps: u64,
    supply: u64,
    es_supply: u64,
    es_vault: u64,
    es_factor: u128,
    /* Configs */
    minting_cost_grow_factor: u128,
    minting_cost: u128,
    reserve_factor: u128,
    es_receiver_factor: u128,
    exchange_time_window: u32,
    es_vesting_divisor: u16,
    padding_1: [u8; 10],
    max_rank: u64,
    ranks: [u64; MAX_RANK],
    order_fee_discount_factors: [u128; MAX_RANK + 1],
    referral_reward_factors: [u128; MAX_RANK + 1],
    receiver: Pubkey,
    reserved_1: [u8; 256],
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
        require_eq!(self.last_minted_at, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.total_minted, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.grow_steps, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.supply, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.es_supply, 0, CoreError::GTStateHasBeenInitialized);
        require_eq!(self.es_factor, 0, CoreError::GTStateHasBeenInitialized);

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

        self.reserve_factor = constants::DEFAULT_GT_RESERVE_FACTOR;
        self.exchange_time_window = constants::DEFAULT_GT_VAULT_TIME_WINDOW;
        self.es_vesting_divisor = constants::DEFAULT_ES_GT_VESTING_DIVISOR;

        Ok(())
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

    pub(crate) fn set_es_recevier_factor(&mut self, factor: u128) -> Result<()> {
        require_gte!(
            constants::MARKET_USD_UNIT,
            factor,
            CoreError::InvalidArgument
        );
        self.es_receiver_factor = factor;
        Ok(())
    }

    pub(crate) fn es_receiver_factor(&self) -> u128 {
        self.es_receiver_factor
    }

    /// Get time window for GT exchange.
    pub fn exchange_time_window(&self) -> u32 {
        self.exchange_time_window
    }

    /// Get GT decimals.
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Get total minted.
    pub fn total_minted(&self) -> u64 {
        self.total_minted
    }

    /// Get GT supply.
    pub fn supply(&self) -> u64 {
        self.supply
    }

    /// Get esGT supply.
    pub fn es_supply(&self) -> u64 {
        self.es_supply
    }

    /// Get es vesting disivor.
    pub fn es_vesting_divisor(&self) -> u16 {
        self.es_vesting_divisor
    }

    /// Get esGT vault.
    pub fn es_vault(&self) -> u64 {
        self.es_vault
    }

    /// Get current vaule of es factor.
    pub fn es_factor(&self) -> u128 {
        self.es_factor
    }

    pub(crate) fn set_exchange_time_window(&mut self, window: u32) -> Result<()> {
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

    /// Sync the esGT factor and mint esGT to the given user.
    ///
    /// # CHECK
    /// - `user` must be owned by this store.
    pub(crate) fn unchecked_sync_es_factor(&mut self, user: &mut UserHeader) -> Result<()> {
        use gmsol_model::utils::apply_factor;

        let gt_amount = u128::from(user.gt.amount());

        let current_factor = self.es_factor;

        if gt_amount == 0 {
            // Must update the user's es factor to the current factor even if the user has no GT.
            //
            // If we don't do this, issues will happen when the user mints some GT and sync the
            // es_factor again:
            //
            // 1. The first minting will not update the user's es_factor because of early return,
            //    which means the user's es_factor is still be zero.
            // 2. When the user mints the second time, the check of `gt_amount == 0` will be false,
            //    and the esGT amount will be calculated as `gt_amount * (current_factor - 0)`, which
            //    is wrong.
            user.gt.es_factor = current_factor;
            return Ok(());
        }

        let user_factor = user.gt.es_factor;
        require_gte!(current_factor, user_factor, CoreError::Internal);

        if current_factor == user_factor {
            return Ok(());
        }

        let diff_factor = current_factor
            .checked_sub(user_factor)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        let amount = apply_factor::<_, { constants::MARKET_DECIMALS }>(&gt_amount, &diff_factor)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        let amount: u64 = amount.try_into()?;

        let next_es_amount = user
            .gt
            .es_amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        let next_es_supply = self
            .es_supply
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        /* The following steps should be infallible. */

        user.gt.es_amount = next_es_amount;
        user.gt.es_factor = current_factor;
        self.es_supply = next_es_supply;

        Ok(())
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

            self.unchecked_sync_es_factor(user)?;

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

    fn validate_gt_reserve(
        &self,
        user: &UserHeader,
        next_gt_amount: Option<u64>,
        next_vesting_es_amount: Option<u64>,
    ) -> Result<()> {
        use gmsol_model::utils::apply_factor;

        let gt_amount = u128::from(next_gt_amount.unwrap_or(user.gt.amount));
        let vesting_es_amount =
            u128::from(next_vesting_es_amount.unwrap_or(user.gt.vesting_es_amount));

        let reserve_gt_amount = apply_factor::<_, { constants::MARKET_DECIMALS }>(
            &vesting_es_amount,
            &self.reserve_factor,
        )
        .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        require_gte!(gt_amount, reserve_gt_amount, CoreError::InvalidArgument);

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

            self.validate_gt_reserve(user, Some(next_amount), None)?;

            let next_supply = self
                .supply
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::Internal))?;

            self.unchecked_sync_es_factor(user)?;

            /* The following steps should be infallible. */

            user.gt.amount = next_amount;
            self.supply = next_supply;

            self.unchecked_update_rank(user);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn get_mint_amount(
        &self,
        size_in_value: u128,
        discount: u128,
    ) -> Result<(u64, u128)> {
        use gmsol_model::utils::apply_factor;

        // Calculate the minting cost to apply.
        let minting_cost = if discount == 0 {
            self.minting_cost
        } else {
            require_gt!(
                constants::MARKET_USD_UNIT,
                discount,
                CoreError::InvalidGTDiscount
            );
            let discounted_factor = constants::MARKET_USD_UNIT - discount;

            apply_factor::<_, { constants::MARKET_DECIMALS }>(
                &self.minting_cost,
                &discounted_factor,
            )
            .ok_or_else(|| error!(CoreError::InvalidGTDiscount))?
        };

        require!(minting_cost != 0, CoreError::InvalidGTConfig);

        let remainder = size_in_value % minting_cost;
        let minted = (size_in_value / minting_cost)
            .try_into()
            .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

        let minted_value = size_in_value - remainder;

        msg!(
            "[GT] will mint {} units of GT with a minting cost of {} per unit GT (in terms of paid order fee), discount = {}",
            minted,
            minting_cost,
            discount,
        );

        Ok((minted, minted_value))
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
    ) -> Result<()> {
        require!(vault.is_initialized(), CoreError::InvalidArgument);

        let amount = vault.confirm()?;

        self.process_es_gt(amount)?;

        Ok(())
    }

    fn process_es_gt(&mut self, amount: u64) -> Result<()> {
        use gmsol_model::utils::{apply_factor, div_to_factor};

        if amount == 0 || self.supply == 0 {
            return Ok(());
        }

        let mut amount = u128::from(amount);
        let amount_for_vault =
            apply_factor::<_, { constants::MARKET_DECIMALS }>(&amount, &self.es_receiver_factor())
                .ok_or_else(|| error!(CoreError::ValueOverflow))?;
        require_gte!(amount, amount_for_vault, CoreError::Internal);
        amount = amount
            .checked_sub(amount_for_vault)
            .ok_or_else(|| error!(CoreError::Internal))?;

        let amount_for_vault: u64 = amount_for_vault.try_into()?;

        let next_es_vault = self
            .es_vault
            .checked_add(amount_for_vault)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        let next_es_supply = self
            .es_supply
            .checked_add(amount_for_vault)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        debug_assert_ne!(self.supply, 0);
        let supply = u128::from(self.supply);

        let delta = div_to_factor::<_, { constants::MARKET_DECIMALS }>(&amount, &supply, false)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        let next_es_factor = self
            .es_factor
            .checked_add(delta)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;

        self.es_vault = next_es_vault;
        self.es_factor = next_es_factor;
        self.es_supply = next_es_supply;
        Ok(())
    }

    /// Request for a vesting.
    ///
    /// # CHECK
    /// - `user` and `vesting` must be initialized and owned by this store.
    /// - `vesting` must belong to the `user`.
    ///
    /// # Errors
    /// - The `user` must have enough amount of esGT.
    ///
    /// # Notes
    /// - This is not an atomic operation.
    #[inline(never)]
    pub(crate) fn unchecked_request_vesting(
        &mut self,
        user: &mut UserHeader,
        vesting: &mut GtVesting,
        amount: u64,
    ) -> Result<()> {
        require_gte!(user.gt.es_amount, amount, CoreError::NotEnoughTokenAmount);

        self.unchecked_update_vesting(user, vesting)?;

        msg!("Vesting state updated");

        let next_es_amount = user
            .gt
            .es_amount
            .checked_sub(amount)
            .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;
        let next_vesting_es_amount = user
            .gt
            .vesting_es_amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        self.validate_gt_reserve(user, None, Some(next_vesting_es_amount))?;

        vesting.add(amount)?;

        user.gt.es_amount = next_es_amount;
        user.gt.vesting_es_amount = next_vesting_es_amount;

        Ok(())
    }

    /// Update vesting state.
    ///
    /// # CHECK
    /// - `user` and `vesting` must be initialized and owned by this store.
    /// - `vesting` msut belong to the `user`.
    ///
    /// # Errors
    ///
    #[inline(never)]
    pub(crate) fn unchecked_update_vesting(
        &mut self,
        user: &mut UserHeader,
        vesting: &mut GtVesting,
    ) -> Result<()> {
        self.unchecked_sync_es_factor(user)?;

        let amount = vesting.advance()?;

        if amount == 0 {
            return Ok(());
        }

        let next_vesting_es_amount = user
            .gt
            .vesting_es_amount
            .checked_sub(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        let next_es_supply = self
            .es_supply
            .checked_sub(amount)
            .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;

        // The process of esGT -> GT does not affect the mint cost and the total minted.
        let next_amount = user
            .gt
            .amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        let next_supply = self
            .supply
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        user.gt.vesting_es_amount = next_vesting_es_amount;
        user.gt.amount = next_amount;
        self.es_supply = next_es_supply;
        self.supply = next_supply;

        self.unchecked_update_rank(user);

        Ok(())
    }

    /// Get esGT vault receiver.
    pub fn receiver(&self) -> Option<Pubkey> {
        if self.receiver == Pubkey::default() {
            None
        } else {
            Some(self.receiver)
        }
    }

    pub(crate) fn validate_receiver(&self, address: &Pubkey) -> Result<()> {
        let receiver = self
            .receiver()
            .ok_or_else(|| error!(CoreError::PreconditionsAreNotMet))?;
        require_eq!(receiver, *address, CoreError::PermissionDenied);
        Ok(())
    }

    pub(crate) fn set_receiver(&mut self, receiver: &Pubkey) -> Result<()> {
        require_neq!(*receiver, Pubkey::default(), CoreError::InvalidArgument);
        self.receiver = *receiver;
        Ok(())
    }

    /// Directly distribute esGT from the esGT vault to the given user as vesting.
    ///
    /// # CHECK
    /// - The `user` and `vesting` must be initialized and owned by this store.
    /// - The owner of the `vesting` must have been authorized to be distributed.
    ///
    /// # Errors
    /// - The esGT vault must have enough amount of esGT.
    ///
    /// # Notes
    /// - This is not an atomic operation.
    pub(crate) fn unchecked_distribute_es_vault(
        &mut self,
        user: &mut UserHeader,
        vesting: &mut GtVesting,
        amount: u64,
    ) -> Result<()> {
        require_neq!(amount, 0, CoreError::InvalidArgument);
        require_gte!(self.es_vault, amount, CoreError::NotEnoughTokenAmount);

        self.unchecked_update_vesting(user, vesting)?;

        let next_es_vault = self
            .es_vault
            .checked_sub(amount)
            .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;
        let next_vesting_es_amount = user
            .gt
            .vesting_es_amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

        vesting.add(amount)?;

        self.es_vault = next_es_vault;
        user.gt.vesting_es_amount = next_vesting_es_amount;

        Ok(())
    }
}

type GtExchangeVaultFlagsMap = bitmaps::Bitmap<MAX_FLAGS>;
type GtExchangeVaultFlagsValue = u8;

/// GT Exchange Vault Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum GtExchangeVaultFlag {
    /// Initialized.
    Intiailized,
    /// Confirmed.
    Comfirmed,
    // CHECK: should have no more than `MAX_FLAGS` of flags.
}

/// GT Exchange Vault.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtExchangeVault {
    pub(crate) bump: u8,
    flags: GtExchangeVaultFlagsValue,
    padding: [u8; 6],
    ts: i64,
    time_window: i64,
    amount: u64,
    pub(crate) store: Pubkey,
    reserved: [u8; 64],
}

impl GtExchangeVault {
    fn get_flag(&self, kind: GtExchangeVaultFlag) -> bool {
        let index = u8::from(kind);
        let map = GtExchangeVaultFlagsMap::from_value(self.flags);
        map.get(usize::from(index))
    }

    fn set_flag(&mut self, kind: GtExchangeVaultFlag, value: bool) -> bool {
        let index = u8::from(kind);
        let mut map = GtExchangeVaultFlagsMap::from_value(self.flags);
        let previous = map.set(usize::from(index), value);
        self.flags = map.into_value();
        previous
    }

    /// Get amount.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Get whether the vault is initialized.
    pub fn is_initialized(&self) -> bool {
        self.get_flag(GtExchangeVaultFlag::Intiailized)
    }

    /// Get whether the vault is comfirmed.
    pub fn is_confirmed(&self) -> bool {
        self.get_flag(GtExchangeVaultFlag::Comfirmed)
    }

    pub(crate) fn init(&mut self, bump: u8, store: &Pubkey, time_window: u32) -> Result<()> {
        require!(!self.is_initialized(), CoreError::PreconditionsAreNotMet);

        require!(time_window != 0, CoreError::InvalidArgument);

        let clock = Clock::get()?;

        self.bump = bump;
        self.ts = clock.unix_timestamp;
        self.store = *store;
        self.set_flag(GtExchangeVaultFlag::Intiailized, true);
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

    fn validate_confirmable(&self) -> Result<()> {
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
        self.set_flag(GtExchangeVaultFlag::Comfirmed, true);
        Ok(self.amount)
    }

    fn validate_depositable(&self) -> Result<()> {
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

type GtExchangeFlagsMap = bitmaps::Bitmap<MAX_FLAGS>;
type GtExchangeFlagsValue = u8;

/// GT Exchange Vault Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum GtExchangeFlag {
    /// Initialized.
    Intiailized,
    // CHECK: should have no more than `MAX_FLAGS` of flags.
}

/// GT Exchange Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtExchange {
    /// Bump.
    pub bump: u8,
    flags: GtExchangeFlagsValue,
    padding: [u8; 6],
    amount: u64,
    /// Owner address.
    pub owner: Pubkey,
    /// Store address.
    pub store: Pubkey,
    /// Vault address.
    pub vault: Pubkey,
    reserved: [u8; 64],
}

impl Default for GtExchange {
    fn default() -> Self {
        use bytemuck::Zeroable;

        Self::zeroed()
    }
}

impl GtExchange {
    fn get_flag(&self, kind: GtExchangeFlag) -> bool {
        let index = u8::from(kind);
        let map = GtExchangeFlagsMap::from_value(self.flags);
        map.get(usize::from(index))
    }

    fn set_flag(&mut self, kind: GtExchangeFlag, value: bool) -> bool {
        let index = u8::from(kind);
        let mut map = GtExchangeFlagsMap::from_value(self.flags);
        let previous = map.set(usize::from(index), value);
        self.flags = map.into_value();
        previous
    }

    /// Get whether the vault is initialized.
    pub fn is_initialized(&self) -> bool {
        self.get_flag(GtExchangeFlag::Intiailized)
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

        self.set_flag(GtExchangeFlag::Intiailized, true);

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

/// Get time window index.
pub fn get_time_window_index(ts: i64, time_window: i64) -> i64 {
    debug_assert!(time_window > 0);
    ts / time_window
}

type GtVestingFlagsMap = bitmaps::Bitmap<MAX_FLAGS>;
type GtVestingFlagsValue = u8;

/// GT Vesting Vault Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum GtVestingFlag {
    /// Initialized.
    Intiailized,
    // CHECK: should have no more than `MAX_FLAGS` of flags.
}

const VESTING_LEN: usize = 1024;

/// GT Vesting.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtVesting {
    pub(crate) bump: u8,
    flags: GtVestingFlagsValue,
    head: u16,
    divisor: u16,
    padding_0: [u8; 2],
    time_window: u32,
    padding_1: [u8; 4],
    time_window_index: i64,
    pub(crate) owner: Pubkey,
    pub(crate) store: Pubkey,
    vesting: [u64; VESTING_LEN],
}

impl GtVesting {
    fn get_flag(&self, kind: GtVestingFlag) -> bool {
        let index = u8::from(kind);
        let map = GtVestingFlagsMap::from_value(self.flags);
        map.get(usize::from(index))
    }

    fn set_flag(&mut self, kind: GtVestingFlag, value: bool) -> bool {
        let index = u8::from(kind);
        let mut map = GtVestingFlagsMap::from_value(self.flags);
        let previous = map.set(usize::from(index), value);
        self.flags = map.into_value();
        previous
    }

    /// Get whether the vault is initialized.
    pub fn is_initialized(&self) -> bool {
        self.get_flag(GtVestingFlag::Intiailized)
    }

    pub(crate) fn init(
        &mut self,
        bump: u8,
        owner: &Pubkey,
        store: &Pubkey,
        divisor: u16,
        time_window: u32,
    ) -> Result<()> {
        require!(!self.is_initialized(), CoreError::PreconditionsAreNotMet);

        require_neq!(divisor, 0, CoreError::InvalidArgument);
        require_gte!(VESTING_LEN, divisor as usize, CoreError::InvalidArgument);

        let clock = Clock::get()?;
        self.time_window_index = get_time_window_index(clock.unix_timestamp, time_window.into());

        self.bump = bump;
        self.owner = *owner;
        self.store = *store;
        self.divisor = divisor;
        self.time_window = time_window;

        self.set_flag(GtVestingFlag::Intiailized, true);

        Ok(())
    }

    fn current_time_window_index(&self) -> Result<i64> {
        let clock = Clock::get()?;
        let current = get_time_window_index(clock.unix_timestamp, self.time_window.into());

        Ok(current)
    }

    fn validate_time_window(&self) -> Result<()> {
        require_eq!(
            self.current_time_window_index()?,
            self.time_window_index,
            CoreError::PreconditionsAreNotMet
        );
        Ok(())
    }

    /// Add amount to the vesting.
    ///
    /// # Notes
    /// - This is not an atomic operation.
    #[inline(never)]
    fn add(&mut self, amount: u64) -> Result<()> {
        require!(self.is_initialized(), CoreError::PreconditionsAreNotMet);

        // Time window must be up-to-date.
        self.validate_time_window()?;

        let d = u64::from(self.divisor);
        let quotient = amount.div_euclid(d);
        let remainder = amount.rem_euclid(d);

        let mut delta;
        for offset in 0..(self.divisor) {
            delta = if offset == 0 {
                quotient + remainder
            } else {
                quotient
            };
            // Since head + offset + 1 <= 2 * VESTING_LEN < u16::MAX, this will never overflow.
            self.add_at_offset(offset, &delta)?;
        }

        Ok(())
    }

    fn offset_to_idx(&self, offset: u16) -> usize {
        (self.head as usize + offset as usize) % VESTING_LEN
    }

    #[inline(never)]
    fn add_at_offset(&mut self, offset: u16, delta: &u64) -> Result<()> {
        let idx = self.offset_to_idx(offset);
        self.vesting[idx] = self.vesting[idx]
            .checked_add(*delta)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        Ok(())
    }

    #[allow(dead_code)]
    fn get_at_offset(&self, offset: u16) -> u64 {
        let idx = self.offset_to_idx(offset);
        self.vesting[idx]
    }

    #[inline(never)]
    fn pop_head(&mut self) -> u64 {
        let amount = &mut self.vesting[usize::from(self.head)];

        if *amount == 0 {
            return 0;
        }

        // Since head + 1 <= VESTING_LEN + 1 < u16::MAX, this will never overflow.
        let next_head = (self.head + 1) % (VESTING_LEN as u16);

        let current = *amount;
        self.head = next_head;
        *amount = 0;

        current
    }

    /// Advance the vesting progress and return the vestable amount.
    #[inline(never)]
    fn advance(&mut self) -> Result<u64> {
        let current = self.current_time_window_index()?;

        require_gte!(
            current,
            self.time_window_index,
            CoreError::PreconditionsAreNotMet
        );

        let mut amount = 0u64;

        for _ in self.time_window_index..current {
            let pop = self.pop_head();
            if pop != 0 {
                amount = amount
                    .checked_add(pop)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            }
        }

        self.time_window_index = current;

        Ok(amount)
    }

    /// Get the owner.
    pub fn owner(&self) -> &Pubkey {
        &self.owner
    }

    /// Get the store.
    pub fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Return whether the vesting is empty.
    pub fn is_empty(&self) -> bool {
        self.vesting[usize::from(self.head)] == 0
    }

    /// Get vesting.
    #[cfg(feature = "utils")]
    pub fn vesting(&self) -> impl Iterator<Item = NonZeroU64> + '_ {
        GtVestingIter {
            vesting: self,
            offset: 0,
        }
    }

    /// Get vestable.
    #[cfg(feature = "utils")]
    pub fn claimable(&self, current: i64) -> u64 {
        let current = get_time_window_index(current, self.time_window as i64);
        (self.time_window_index..current)
            .enumerate()
            .map(|(offset, _)| self.get_at_offset(offset as u16))
            .take_while(|amount| *amount != 0)
            .sum()
    }
}

impl gmsol_utils::InitSpace for GtVesting {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for GtVesting {
    const SEED: &'static [u8] = b"gt_vesting";
}

#[cfg(feature = "utils")]
struct GtVestingIter<'a> {
    vesting: &'a GtVesting,
    offset: u16,
}

#[cfg(feature = "utils")]
impl<'a> Iterator for GtVestingIter<'a> {
    type Item = NonZeroU64;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.vesting.get_at_offset(self.offset);
        if value == 0 {
            None
        } else {
            self.offset += 1;
            NonZeroU64::new(value)
        }
    }
}
