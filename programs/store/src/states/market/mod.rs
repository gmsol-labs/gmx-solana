//! ## Market
//! A [`Market`](crate::states::Market) in GMX-Solana is defined by three tokens:
//!
//! - **Index Token**: The underlying asset that serves as the trading instrument. The price movements
//!   of this token determine the profit or loss of positions. It does not need to be a real token.
//! - **Long Token**: The token used to:
//!   - Collateralize long and short positions
//!   - Pay profits to long position holders
//! - **Short Token**: The token used to:
//!   - Collateralize long and short positions
//!   - Pay profits to short position holders
//!
//! Long token and short token can be the same token, in which case the market
//! is called a *single-token market*.
//!
//! Liquidity Providers (LPs) can provide liquidity to the market by depositing pool tokens (long token
//! and short token) in exchange for market tokens. These market tokens represent the LP's proportional
//! share of the market's liquidity pool. The deposited tokens are held in shared token accounts called
//! *Market Vaults*, with deposited amounts for this market tracked in the market state. LPs can later
//! redeem their market tokens back for the underlying collateral tokens through withdrawal instructions.
//!
//! Traders can open long or short positions using either token as collateral. When opening a position,
//! the trader deposits collateral tokens and specifies the desired leverage. The position's profit or
//! loss is determined by price movements of the index token. The loss is incurred in the collateral
//! token used to open the position while the profit is realized in the long token and short token
//! respectively.

use std::str::FromStr;

use anchor_lang::{prelude::*, Bump};
use anchor_spl::token::Mint;
use borsh::{BorshDeserialize, BorshSerialize};
use config::MarketConfigFlag;
use gmsol_model::{
    num::Unsigned, price::Prices, Balance, BaseMarket, BaseMarketExt, ClockKind, Delta, PoolKind,
};
use gmsol_utils::{
    market::{MarketError, MarketFlag, MAX_MARKET_FLAGS},
    pubkey::{optional_address, DEFAULT_PUBKEY},
};
use pool::cancel_amounts;
use revertible::RevertibleBuffer;
use virtual_inventory::VirtualInventory;

use crate::{
    utils::fixed_str::{bytes_to_fixed_str, fixed_str_to_bytes},
    CoreError, ModelError,
};

use super::{Factor, InitSpace, Oracle, Seed};

use self::{
    config::{MarketConfig, MarketConfigBuffer, MarketConfigKey},
    pool::{Pool, Pools},
};

pub use gmsol_utils::market::{HasMarketMeta, MarketMeta};
pub use model::AsLiquidityMarket;

/// Market Utils.
pub mod utils;

/// Clock ops.
pub mod clock;

/// Market Config.
pub mod config;

/// Revertible Market Operations.
pub mod revertible;

/// Pool.
pub mod pool;

/// Market Status.
pub mod status;

/// Virtual Inventory.
pub mod virtual_inventory;

mod model;

const MAX_NAME_LEN: usize = 64;

/// Market.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Market {
    version: u8,
    /// Bump Seed.
    pub(crate) bump: u8,
    flags: MarketFlagContainer,
    padding: [u8; 13],
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    name: [u8; MAX_NAME_LEN],
    pub(crate) meta: MarketMeta,
    /// Store.
    pub store: Pubkey,
    config: MarketConfig,
    indexer: Indexer,
    state: State,
    buffer: RevertibleBuffer,
    virtual_inventory_for_swaps: Pubkey,
    virtual_inventory_for_positions: Pubkey,
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 192],
}

#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct State {
    pools: Pools,
    clocks: Clocks,
    other: OtherState,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 1024],
}

impl Bump for Market {
    fn seed(&self) -> u8 {
        self.bump
    }
}

impl Seed for Market {
    const SEED: &'static [u8] = b"market";
}

impl InitSpace for Market {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Default for Market {
    fn default() -> Self {
        use bytemuck::Zeroable;
        Self::zeroed()
    }
}

impl AsRef<Market> for Market {
    fn as_ref(&self) -> &Market {
        self
    }
}

impl Market {
    /// Find PDA for [`Market`] account.
    pub fn find_market_address(
        store: &Pubkey,
        token: &Pubkey,
        store_program_id: &Pubkey,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::SEED, store.as_ref(), token.as_ref()],
            store_program_id,
        )
    }

    /// Initialize the market.
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        &mut self,
        bump: u8,
        store: Pubkey,
        name: &str,
        market_token_mint: Pubkey,
        index_token_mint: Pubkey,
        long_token_mint: Pubkey,
        short_token_mint: Pubkey,
        is_enabled: bool,
    ) -> Result<()> {
        self.bump = bump;
        self.store = store;
        self.name = fixed_str_to_bytes(name)?;
        self.set_enabled(is_enabled);
        self.meta.market_token_mint = market_token_mint;
        self.meta.index_token_mint = index_token_mint;
        self.meta.long_token_mint = long_token_mint;
        self.meta.short_token_mint = short_token_mint;
        let is_pure = self.meta.long_token_mint == self.meta.short_token_mint;
        self.set_flag(MarketFlag::Pure, is_pure);
        self.state.pools.init(is_pure);
        self.state.clocks.init_to_current()?;
        self.config.init();

        // Initialize buffer.
        self.buffer.init();
        Ok(())
    }

    /// Get meta.
    pub fn meta(&self) -> &MarketMeta {
        &self.meta
    }

    /// Get validated meta.
    pub fn validated_meta(&self, store: &Pubkey) -> Result<&MarketMeta> {
        self.validate(store)?;
        Ok(self.meta())
    }

    /// Get name.
    pub fn name(&self) -> Result<&str> {
        bytes_to_fixed_str(&self.name)
    }

    /// Description.
    pub fn description(&self) -> Result<String> {
        let name = self.name()?;
        Ok(format!(
            "Market {{ name = {name}, token = {}}}",
            self.meta.market_token_mint
        ))
    }

    /// Get flag.
    pub fn flag(&self, flag: MarketFlag) -> bool {
        self.flags.get_flag(flag)
    }

    /// Set flag.
    ///
    /// Return the previous value.
    pub fn set_flag(&mut self, flag: MarketFlag, value: bool) -> bool {
        self.flags.set_flag(flag, value)
    }

    /// Is this market a pure market, i.e., a single token market.
    pub fn is_pure(&self) -> bool {
        self.flag(MarketFlag::Pure)
    }

    /// Is this market enabled.
    pub fn is_enabled(&self) -> bool {
        self.flag(MarketFlag::Enabled)
    }

    /// Set enabled.
    ///
    /// Return previous value.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        self.set_flag(MarketFlag::Enabled, enabled)
    }

    /// Is ADL enabled.
    pub fn is_adl_enabled(&self, is_long: bool) -> bool {
        if is_long {
            self.flag(MarketFlag::AutoDeleveragingEnabledForLong)
        } else {
            self.flag(MarketFlag::AutoDeleveragingEnabledForShort)
        }
    }

    /// Set ADL enabled.
    ///
    /// Return previous value.
    pub fn set_adl_enabled(&mut self, is_long: bool, enabled: bool) -> bool {
        if is_long {
            self.set_flag(MarketFlag::AutoDeleveragingEnabledForLong, enabled)
        } else {
            self.set_flag(MarketFlag::AutoDeleveragingEnabledForShort, enabled)
        }
    }

    /// Is GT Minting enabled.
    pub fn is_gt_minting_enabled(&self) -> bool {
        self.flag(MarketFlag::GTEnabled)
    }

    /// Set whether the GT minting is enabled.
    ///
    /// Return the previous value.
    pub fn set_is_gt_minting_enbaled(&mut self, enabled: bool) -> bool {
        self.set_flag(MarketFlag::GTEnabled, enabled)
    }

    /// Get pool of the given kind.
    #[inline]
    pub fn pool(&self, kind: PoolKind) -> Option<Pool> {
        self.state.pools.get(kind).map(|s| s.pool()).copied()
    }

    /// Try to get pool of the given kind.
    pub fn try_pool(&self, kind: PoolKind) -> gmsol_model::Result<&Pool> {
        Ok(self
            .state
            .pools
            .get(kind)
            .ok_or(gmsol_model::Error::MissingPoolKind(kind))?
            .pool())
    }

    /// Get clock of the given kind.
    pub fn clock(&self, kind: ClockKind) -> Option<i64> {
        self.state.clocks.get(kind).copied()
    }

    fn clocks(&self) -> &Clocks {
        &self.state.clocks
    }

    /// Validate the market.
    pub fn validate(&self, store: &Pubkey) -> Result<()> {
        require_keys_eq!(*store, self.store, CoreError::StoreMismatched);
        require!(self.is_enabled(), CoreError::DisabledMarket);
        Ok(())
    }

    /// Get config.
    pub fn get_config(&self, key: &str) -> Result<&Factor> {
        let key = MarketConfigKey::from_str(key)
            .map_err(|_| error!(CoreError::InvalidMarketConfigKey))?;
        self.get_config_by_key(key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get config by key.
    #[inline]
    pub fn get_config_by_key(&self, key: MarketConfigKey) -> Option<&Factor> {
        self.config.get(key)
    }

    /// Get config mutably.
    pub fn get_config_mut(&mut self, key: &str) -> Result<&mut Factor> {
        let key = MarketConfigKey::from_str(key)
            .map_err(|_| error!(CoreError::InvalidMarketConfigKey))?;
        self.config
            .get_mut(key)
            .ok_or_else(|| error!(CoreError::Unimplemented))
    }

    /// Get config flag.
    pub fn get_config_flag(&self, key: &str) -> Result<bool> {
        let key = MarketConfigFlag::from_str(key)
            .map_err(|_| error!(CoreError::InvalidMarketConfigKey))?;
        Ok(self.get_config_flag_by_key(key))
    }

    /// Get config flag by key.
    #[inline]
    pub fn get_config_flag_by_key(&self, key: MarketConfigFlag) -> bool {
        self.config.flag(key)
    }

    /// Set config flag.
    ///
    /// Returns previous value.
    pub fn set_config_flag(&mut self, key: &str, value: bool) -> Result<bool> {
        let key = MarketConfigFlag::from_str(key)
            .map_err(|_| error!(CoreError::InvalidMarketConfigKey))?;
        Ok(self.config.set_flag(key, value))
    }

    /// Get other market state.
    pub fn state(&self) -> &OtherState {
        &self.state.other
    }

    /// Get market indexer.
    pub fn indexer(&self) -> &Indexer {
        &self.indexer
    }

    /// Get market indexer mutably.
    pub fn indexer_mut(&mut self) -> &mut Indexer {
        &mut self.indexer
    }

    /// Update config with buffer.
    pub fn update_config_with_buffer(&mut self, buffer: &MarketConfigBuffer) -> Result<()> {
        for entry in buffer.iter() {
            let key = entry.key()?;
            let current_value = self
                .config
                .get_mut(key)
                .ok_or_else(|| error!(CoreError::Unimplemented))?;
            let new_value = entry.value();
            *current_value = new_value;
        }
        Ok(())
    }

    /// Get prices from oracle.
    pub fn prices(&self, oracle: &Oracle) -> Result<Prices<u128>> {
        oracle.market_prices(self)
    }

    /// Get max pool value for deposit.
    pub fn max_pool_value_for_deposit(&self, is_long_token: bool) -> gmsol_model::Result<Factor> {
        if is_long_token {
            Ok(self.config.max_pool_value_for_deposit_for_long_token)
        } else {
            Ok(self.config.max_pool_value_for_deposit_for_short_token)
        }
    }

    /// As a liquidity market.
    pub fn as_liquidity_market<'a>(
        &'a self,
        market_token: &'a Mint,
    ) -> AsLiquidityMarket<'a, Self> {
        AsLiquidityMarket::new(self, market_token)
    }

    /// Validate that this market is shiftable to the target market.
    pub fn validate_shiftable(&self, target: &Self) -> Result<()> {
        // Currently we only support the shift between markets with
        // with the same long tokens and short tokens.
        //
        // It should be possible to allow shift between markets with the compatible tokens in the future,
        // for example, allowing shifting from BTC[WSOL-USDC] to SOL[USDC-WSOL].

        require_keys_eq!(
            self.meta().long_token_mint,
            target.meta().long_token_mint,
            CoreError::TokenMintMismatched,
        );

        require_keys_eq!(
            self.meta().short_token_mint,
            target.meta().short_token_mint,
            CoreError::TokenMintMismatched,
        );

        Ok(())
    }

    /// Returns the address of virtual inventory for swaps.
    pub fn virtual_inventory_for_swaps(&self) -> Option<&Pubkey> {
        optional_address(&self.virtual_inventory_for_swaps)
    }

    /// Join a virtual inventory for swaps.
    ///
    /// # CHECK
    /// - The provided [`VirtualInventory`] must be used as a
    ///   virtual inventory for swaps.
    pub fn join_virtual_inventory_for_swaps_unchecked(
        &mut self,
        address: &Pubkey,
        virtual_inventory_for_swaps: &mut VirtualInventory,
    ) -> Result<()> {
        require_keys_neq!(*address, DEFAULT_PUBKEY);
        require!(
            self.virtual_inventory_for_swaps().is_none(),
            CoreError::PreconditionsAreNotMet
        );
        self.virtual_inventory_for_swaps = *address;
        let liquidity_pool = self.liquidity_pool().map_err(ModelError::from)?;
        virtual_inventory_for_swaps.join_unchecked(Delta::new_both_sides(
            true,
            &(liquidity_pool
                .long_amount()
                .map_err(ModelError::from)?
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
            &(liquidity_pool
                .short_amount()
                .map_err(ModelError::from)?
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
        ))?;
        Ok(())
    }

    /// Leave the virtual inventory for swaps.
    ///
    /// # CHECK
    /// - The provided [`VirtualInventory`] must be the associated one.
    pub fn leave_virtual_inventory_for_swaps_unchecked(
        &mut self,
        virtual_inventory_for_swaps: &mut VirtualInventory,
    ) -> Result<()> {
        require!(
            self.virtual_inventory_for_swaps().is_some(),
            CoreError::PreconditionsAreNotMet
        );
        let liquidity_pool = self.liquidity_pool().map_err(ModelError::from)?;
        virtual_inventory_for_swaps.leave_unchecked(Delta::new_both_sides(
            true,
            &(liquidity_pool
                .long_amount()
                .map_err(ModelError::from)?
                .to_opposite_signed()
                .map_err(ModelError::from)?),
            &(liquidity_pool
                .short_amount()
                .map_err(ModelError::from)?
                .to_opposite_signed()
                .map_err(ModelError::from)?),
        ))?;
        self.virtual_inventory_for_swaps = DEFAULT_PUBKEY;
        Ok(())
    }

    /// Leave a disabled virtual inventory.
    ///
    /// # CHECK
    /// - The address and the provided [`VirtualInventory`] must match.
    pub fn leave_disabled_virtual_inventory_unchecked(
        &mut self,
        address: &Pubkey,
        virtual_inventory: &mut VirtualInventory,
    ) -> Result<()> {
        require!(
            virtual_inventory.is_disabled(),
            CoreError::PreconditionsAreNotMet
        );
        if self.virtual_inventory_for_swaps() == Some(address) {
            self.virtual_inventory_for_swaps = DEFAULT_PUBKEY;
        }
        if self.virtual_inventory_for_positions() == Some(address) {
            self.virtual_inventory_for_positions = DEFAULT_PUBKEY;
        }
        Ok(())
    }

    /// Returns the address of virtual inventory for positions.
    pub fn virtual_inventory_for_positions(&self) -> Option<&Pubkey> {
        optional_address(&self.virtual_inventory_for_positions)
    }

    /// Join a virtual inventory for positions.
    ///
    /// # CHECK
    /// - The provided [`VirtualInventory`] must be used as a
    ///   virtual inventory for positions.
    pub fn join_virtual_inventory_for_positions_unchecked(
        &mut self,
        address: &Pubkey,
        virtual_inventory_for_positions: &mut VirtualInventory,
    ) -> Result<()> {
        require_keys_neq!(*address, DEFAULT_PUBKEY);
        require!(
            self.virtual_inventory_for_positions().is_none(),
            CoreError::PreconditionsAreNotMet
        );
        self.virtual_inventory_for_positions = *address;
        let open_interest = self.open_interest().map_err(ModelError::from)?;
        let (long_amount, short_amount) = cancel_amounts(
            open_interest.long_amount().map_err(ModelError::from)?,
            open_interest.short_amount().map_err(ModelError::from)?,
        );
        virtual_inventory_for_positions.join_unchecked(Delta::new_both_sides(
            true,
            &(long_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
            &(short_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
        ))?;
        virtual_inventory_for_positions.cancel_amounts_unchecked()?;
        Ok(())
    }

    /// Leave the virtual inventory for positions.
    ///
    /// # CHECK
    /// - The provided [`VirtualInventory`] must be the associated one.
    pub fn leave_virtual_inventory_for_positions_unchecked(
        &mut self,
        virtual_inventory_for_positions: &mut VirtualInventory,
    ) -> Result<()> {
        require!(
            self.virtual_inventory_for_positions().is_some(),
            CoreError::PreconditionsAreNotMet
        );
        let open_interest = self.open_interest().map_err(ModelError::from)?;
        let (long_amount, short_amount) = cancel_amounts(
            open_interest.long_amount().map_err(ModelError::from)?,
            open_interest.short_amount().map_err(ModelError::from)?,
        );
        virtual_inventory_for_positions.leave_unchecked(Delta::new_both_sides(
            false,
            &(long_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
            &(short_amount
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?),
        ))?;
        virtual_inventory_for_positions.cancel_amounts_unchecked()?;
        self.virtual_inventory_for_positions = DEFAULT_PUBKEY;
        Ok(())
    }
}

gmsol_utils::flags!(MarketFlag, MAX_MARKET_FLAGS, u8);

/// Market State.
#[zero_copy]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OtherState {
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 16],
    rev: u64,
    trade_count: u64,
    long_token_balance: u64,
    short_token_balance: u64,
    funding_factor_per_second: i128,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 256],
}

impl OtherState {
    /// Get long token balance.
    pub fn long_token_balance_raw(&self) -> u64 {
        self.long_token_balance
    }

    /// Get short token balance.
    pub fn short_token_balance_raw(&self) -> u64 {
        self.short_token_balance
    }

    /// Get funding factor per second.
    pub fn funding_factor_per_second(&self) -> i128 {
        self.funding_factor_per_second
    }

    /// Get current trade count.
    pub fn trade_count(&self) -> u64 {
        self.trade_count
    }

    /// Next trade id.
    pub fn next_trade_id(&mut self) -> Result<u64> {
        let next_id = self
            .trade_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.trade_count = next_id;
        Ok(next_id)
    }
}

impl HasMarketMeta for Market {
    fn is_pure(&self) -> bool {
        self.is_pure()
    }

    fn market_meta(&self) -> &MarketMeta {
        &self.meta
    }
}

/// Market clocks.
#[zero_copy]
#[derive(BorshSerialize, BorshDeserialize, InitSpace)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Clocks {
    #[cfg_attr(feature = "debug", debug(skip))]
    padding: [u8; 8],
    rev: u64,
    /// Price impact distribution clock.
    price_impact_distribution: i64,
    /// Borrowing clock.
    borrowing: i64,
    /// Funding clock.
    funding: i64,
    /// ADL updated clock for long.
    adl_for_long: i64,
    /// ADL updated clock for short.
    adl_for_short: i64,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [i64; 3],
}

impl Clocks {
    fn init_to_current(&mut self) -> Result<()> {
        let current = Clock::get()?.unix_timestamp;
        self.price_impact_distribution = current;
        self.borrowing = current;
        self.funding = current;
        Ok(())
    }

    fn get(&self, kind: ClockKind) -> Option<&i64> {
        let clock = match kind {
            ClockKind::PriceImpactDistribution => &self.price_impact_distribution,
            ClockKind::Borrowing => &self.borrowing,
            ClockKind::Funding => &self.funding,
            ClockKind::AdlForLong => &self.adl_for_long,
            ClockKind::AdlForShort => &self.adl_for_short,
            _ => return None,
        };
        Some(clock)
    }

    fn get_mut(&mut self, kind: ClockKind) -> Option<&mut i64> {
        let clock = match kind {
            ClockKind::PriceImpactDistribution => &mut self.price_impact_distribution,
            ClockKind::Borrowing => &mut self.borrowing,
            ClockKind::Funding => &mut self.funding,
            ClockKind::AdlForLong => &mut self.adl_for_long,
            ClockKind::AdlForShort => &mut self.adl_for_short,
            _ => return None,
        };
        Some(clock)
    }
}

/// Market indexer.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Indexer {
    trade_count: u64,
    deposit_count: u64,
    withdrawal_count: u64,
    order_count: u64,
    shift_count: u64,
    glv_deposit_count: u64,
    glv_withdrawal_count: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 8],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl Indexer {
    /// Get current deposit count.
    pub fn deposit_count(&self) -> u64 {
        self.deposit_count
    }

    /// Get current withdrawal count.
    pub fn withdrawal_count(&self) -> u64 {
        self.withdrawal_count
    }

    /// Get current order count.
    pub fn order_count(&self) -> u64 {
        self.order_count
    }

    /// Get current shift count.
    pub fn shift_count(&self) -> u64 {
        self.shift_count
    }

    /// Get current GLV deposit count.
    pub fn glv_deposit_count(&self) -> u64 {
        self.glv_deposit_count
    }

    /// Get current GLV withdrawal count.
    pub fn glv_withdrawal_count(&self) -> u64 {
        self.glv_withdrawal_count
    }

    /// Next deposit id.
    pub fn next_deposit_id(&mut self) -> Result<u64> {
        let next_id = self
            .deposit_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.deposit_count = next_id;
        Ok(next_id)
    }

    /// Next withdrawal id.
    pub fn next_withdrawal_id(&mut self) -> Result<u64> {
        let next_id = self
            .withdrawal_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.withdrawal_count = next_id;
        Ok(next_id)
    }

    /// Next order id.
    pub fn next_order_id(&mut self) -> Result<u64> {
        let next_id = self
            .order_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.order_count = next_id;
        Ok(next_id)
    }

    /// Next shift id.
    pub fn next_shift_id(&mut self) -> Result<u64> {
        let next_id = self
            .shift_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.shift_count = next_id;
        Ok(next_id)
    }

    /// Next GLV deposit id.
    pub fn next_glv_deposit_id(&mut self) -> Result<u64> {
        let next_id = self
            .glv_deposit_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.glv_deposit_count = next_id;
        Ok(next_id)
    }

    /// Next GLV withdrawal id.
    pub fn next_glv_withdrawal_id(&mut self) -> Result<u64> {
        let next_id = self
            .glv_withdrawal_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.glv_withdrawal_count = next_id;
        Ok(next_id)
    }
}

impl From<MarketError> for CoreError {
    fn from(err: MarketError) -> Self {
        msg!("Market Error: {}", err);
        match err {
            MarketError::NotACollateralToken => Self::InvalidArgument,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventClocks, EventOtherState};

    #[test]
    fn test_event_clocks() {
        let clocks = Clocks {
            padding: Default::default(),
            rev: u64::MAX,
            price_impact_distribution: i64::MAX,
            borrowing: i64::MAX,
            funding: i64::MAX,
            adl_for_long: i64::MAX,
            adl_for_short: i64::MAX,
            reserved: Default::default(),
        };

        let event_clocks = EventClocks {
            padding: clocks.padding,
            rev: clocks.rev,
            price_impact_distribution: clocks.price_impact_distribution,
            borrowing: clocks.borrowing,
            funding: clocks.funding,
            adl_for_long: clocks.adl_for_long,
            adl_for_short: clocks.adl_for_short,
            reserved: clocks.reserved,
        };

        let mut data = Vec::with_capacity(Pool::INIT_SPACE);
        clocks
            .serialize(&mut data)
            .expect("failed to serialize `Clocks`");

        let mut event_data = Vec::with_capacity(Pool::INIT_SPACE);
        event_clocks
            .serialize(&mut event_data)
            .expect("failed to serialize `EventClocks`");

        assert_eq!(data, event_data);
    }

    #[test]
    fn test_event_other_state() {
        let clocks = OtherState {
            padding: Default::default(),
            rev: u64::MAX,
            trade_count: u64::MAX,
            long_token_balance: u64::MAX,
            short_token_balance: u64::MAX,
            funding_factor_per_second: i128::MAX,
            reserved: [0; 256],
        };

        let event_clocks = EventOtherState {
            padding: clocks.padding,
            rev: clocks.rev,
            trade_count: clocks.trade_count,
            long_token_balance: clocks.long_token_balance,
            short_token_balance: clocks.short_token_balance,
            funding_factor_per_second: clocks.funding_factor_per_second,
            reserved: clocks.reserved,
        };

        let mut data = Vec::with_capacity(Pool::INIT_SPACE);
        clocks
            .serialize(&mut data)
            .expect("failed to serialize `OtherState`");

        let mut event_data = Vec::with_capacity(Pool::INIT_SPACE);
        event_clocks
            .serialize(&mut event_data)
            .expect("failed to serialize `EventOtherState`");

        assert_eq!(data, event_data);
    }
}
