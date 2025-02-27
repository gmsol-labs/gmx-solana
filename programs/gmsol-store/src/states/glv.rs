use std::{
    borrow::{Borrow, BorrowMut},
    collections::BTreeSet,
};

use anchor_lang::prelude::*;
use anchor_spl::token_interface;
use gmsol_utils::InitSpace;

use crate::{
    constants,
    events::{GlvDepositRemoved, GlvWithdrawalRemoved, ShiftRemoved},
    states::{Deposit, Market},
    CoreError,
};

use super::{
    common::{
        action::{Action, ActionHeader, Closable},
        swap::{unpack_markets, HasSwapParams, SwapParams},
        token::{TokenAndAccount, TokensCollector},
    },
    deposit::DepositParams,
    shift, Seed, Shift, TokenMapAccess,
};

const MAX_ALLOWED_NUMBER_OF_MARKETS: usize = 96;
/// Max number of flags.
pub const MAX_FLAGS: usize = 8;

/// Glv.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Glv {
    version: u8,
    /// Bump seed.
    pub(crate) bump: u8,
    bump_bytes: [u8; 1],
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 3],
    /// Index.
    pub(crate) index: u16,
    pub(crate) store: Pubkey,
    pub(crate) glv_token: Pubkey,
    pub(crate) long_token: Pubkey,
    pub(crate) short_token: Pubkey,
    shift_last_executed_at: i64,
    pub(crate) min_tokens_for_first_deposit: u64,
    shift_min_interval_secs: u32,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 4],
    shift_max_price_impact_factor: u128,
    shift_min_value: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 256],
    /// Market config map with market token addresses as keys.
    markets: GlvMarkets,
}

gmsol_utils::fixed_map!(
    GlvMarkets,
    Pubkey,
    crate::utils::pubkey::to_bytes,
    GlvMarketConfig,
    MAX_ALLOWED_NUMBER_OF_MARKETS,
    12
);

impl Seed for Glv {
    const SEED: &'static [u8] = b"glv";
}

impl InitSpace for Glv {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Glv {
    /// GLV token seed.
    pub const GLV_TOKEN_SEED: &'static [u8] = b"glv_token";

    /// Max allowed number of markets.
    pub const MAX_ALLOWED_NUMBER_OF_MARKETS: usize = MAX_ALLOWED_NUMBER_OF_MARKETS;

    /// Find GLV token address.
    pub fn find_glv_token_pda(store: &Pubkey, index: u16, program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::GLV_TOKEN_SEED, store.as_ref(), &index.to_le_bytes()],
            program_id,
        )
    }

    /// Find GLV address.
    pub fn find_glv_pda(glv_token: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, glv_token.as_ref()], program_id)
    }

    pub(crate) fn signer_seeds(&self) -> [&[u8]; 3] {
        [Self::SEED, self.glv_token().as_ref(), &self.bump_bytes]
    }

    pub(crate) fn vec_signer_seeds(&self) -> Vec<Vec<u8>> {
        vec![
            Self::SEED.to_vec(),
            self.glv_token.to_bytes().to_vec(),
            self.bump_bytes.to_vec(),
        ]
    }

    /// Initialize the [`Glv`] account.
    ///
    /// # CHECK
    /// - The [`Glv`] account must be uninitialized.
    /// - The `bump` must be the bump deriving the address of the [`Glv`] account.
    /// - The `glv_token` must be used to derive the address of the [`Glv`] account.
    /// - The market tokens must be valid and unique, and their corresponding markets
    ///   must use the given tokens as long token and short token.
    /// - The `store` must be the address of the store owning the corresponding markets.
    ///
    /// # Errors
    /// - The `glv_token` address must be derived from [`GLV_TOKEN_SEED`](Self::GLV_TOKEN_SEED), `store` and `index`.
    /// - The total number of the market tokens must not exceed the max allowed number of markets.
    pub(crate) fn unchecked_init(
        &mut self,
        bump: u8,
        index: u16,
        store: &Pubkey,
        glv_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        market_tokens: &BTreeSet<Pubkey>,
    ) -> Result<()> {
        let expected_glv_token = Self::find_glv_token_pda(store, index, &crate::ID).0;
        require_keys_eq!(expected_glv_token, *glv_token, CoreError::InvalidArgument);

        self.version = 0;
        self.bump = bump;
        self.bump_bytes = [bump];
        self.index = index;
        self.store = *store;
        self.glv_token = *glv_token;
        self.long_token = *long_token;
        self.short_token = *short_token;

        self.shift_min_interval_secs = constants::DEFAULT_GLV_MIN_SHIFT_INTERVAL_SECS;
        self.shift_max_price_impact_factor = constants::DEFAULT_GLV_MAX_SHIFT_PRICE_IMPACT_FACTOR;
        self.shift_min_value = constants::DEFAULT_GLV_MIN_SHIFT_VALUE;

        require_gte!(
            Self::MAX_ALLOWED_NUMBER_OF_MARKETS,
            market_tokens.len(),
            CoreError::ExceedMaxLengthLimit
        );

        for market_token in market_tokens {
            self.markets
                .insert_with_options(market_token, Default::default(), true)?;
        }
        Ok(())
    }

    pub(crate) fn process_and_validate_markets_for_init<'info>(
        markets: &'info [AccountInfo<'info>],
        store: &Pubkey,
    ) -> Result<(Pubkey, Pubkey, BTreeSet<Pubkey>)> {
        let mut tokens = None;

        let mut market_tokens = BTreeSet::default();
        for market in unpack_markets(markets) {
            let market = market?;
            let market = market.load()?;
            let meta = market.validated_meta(store)?;
            match &mut tokens {
                Some((long_token, short_token)) => {
                    require_keys_eq!(
                        *long_token,
                        meta.long_token_mint,
                        CoreError::TokenMintMismatched
                    );
                    require_keys_eq!(
                        *short_token,
                        meta.short_token_mint,
                        CoreError::TokenMintMismatched
                    );
                }
                none => {
                    *none = Some((meta.long_token_mint, meta.short_token_mint));
                }
            }
            require!(
                market_tokens.insert(meta.market_token_mint),
                CoreError::InvalidArgument
            );
        }

        if let Some((long_token, short_token)) = tokens {
            require_eq!(markets.len(), market_tokens.len(), CoreError::Internal);
            Ok((long_token, short_token, market_tokens))
        } else {
            err!(CoreError::InvalidArgument)
        }
    }

    /// Get the version of the [`Glv`] account format.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Get the index of the glv token.
    pub fn index(&self) -> u16 {
        self.index
    }

    /// Get the store address.
    pub fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Get the GLV token address.
    pub fn glv_token(&self) -> &Pubkey {
        &self.glv_token
    }

    /// Get the long token address.
    pub fn long_token(&self) -> &Pubkey {
        &self.long_token
    }

    /// Get the short token address.
    pub fn short_token(&self) -> &Pubkey {
        &self.short_token
    }

    pub(crate) fn update_config(&mut self, params: &UpdateGlvParams) -> Result<()> {
        if let Some(amount) = params.min_tokens_for_first_deposit {
            require_neq!(
                self.min_tokens_for_first_deposit,
                amount,
                CoreError::PreconditionsAreNotMet
            );
            self.min_tokens_for_first_deposit = amount;
        }

        if let Some(secs) = params.shift_min_interval_secs {
            require_neq!(
                self.shift_min_interval_secs,
                secs,
                CoreError::PreconditionsAreNotMet
            );
            self.shift_min_interval_secs = secs;
        }

        if let Some(factor) = params.shift_max_price_impact_factor {
            require_neq!(
                self.shift_max_price_impact_factor,
                factor,
                CoreError::PreconditionsAreNotMet
            );
            self.shift_max_price_impact_factor = factor;
        }

        if let Some(value) = params.shift_min_value {
            require_neq!(
                self.shift_min_value,
                value,
                CoreError::PreconditionsAreNotMet
            );
            self.shift_min_value = value;
        }

        Ok(())
    }

    pub(crate) fn insert_market(&mut self, store: &Pubkey, market: &Market) -> Result<()> {
        let meta = market.validated_meta(store)?;

        require_keys_eq!(
            meta.long_token_mint,
            self.long_token,
            CoreError::InvalidArgument
        );

        require_keys_eq!(
            meta.short_token_mint,
            self.short_token,
            CoreError::InvalidArgument
        );

        let market_token = meta.market_token_mint;
        self.markets
            .insert_with_options(&market_token, GlvMarketConfig::default(), true)?;

        Ok(())
    }

    /// Remove market from the GLV.
    ///
    /// # CHECK
    /// - The balance of the vault must be zero.
    pub(crate) fn unchecked_remove_market(&mut self, market_token: &Pubkey) -> Result<()> {
        let config = self
            .market_config(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?;

        require!(
            !config.get_flag(GlvMarketFlag::IsDepositAllowed),
            CoreError::PreconditionsAreNotMet
        );

        require!(
            self.markets.remove(market_token).is_some(),
            CoreError::Internal
        );

        Ok(())
    }

    /// Get all market tokens.
    pub fn market_tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.markets
            .entries()
            .map(|(key, _)| Pubkey::new_from_array(*key))
    }

    /// Get the total number of markets.
    pub fn num_markets(&self) -> usize {
        self.markets.len()
    }

    /// Return whether the given market token is contained in this GLV.
    pub fn contains(&self, market_token: &Pubkey) -> bool {
        self.markets.get(market_token).is_some()
    }

    /// Get [`GlvMarketConfig`] for the given market.
    pub fn market_config(&self, market_token: &Pubkey) -> Option<&GlvMarketConfig> {
        self.markets.get(market_token)
    }

    pub(crate) fn update_market_config(
        &mut self,
        market_token: &Pubkey,
        max_amount: Option<u64>,
        max_value: Option<u128>,
    ) -> Result<()> {
        let config = self
            .markets
            .get_mut(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        if let Some(amount) = max_amount {
            config.max_amount = amount;
        }
        if let Some(value) = max_value {
            config.max_value = value;
        }
        Ok(())
    }

    pub(crate) fn toggle_market_config_flag(
        &mut self,
        market_token: &Pubkey,
        flag: GlvMarketFlag,
        enable: bool,
    ) -> Result<bool> {
        self.markets
            .get_mut(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?
            .toggle_flag(flag, enable)
    }

    /// Create a new [`TokensCollector`].
    pub fn tokens_collector(&self, action: Option<&impl HasSwapParams>) -> TokensCollector {
        TokensCollector::new(action, self.num_markets())
    }

    /// Split remaining accounts.
    pub(crate) fn validate_and_split_remaining_accounts<'info>(
        &self,
        store: &Pubkey,
        remaining_accounts: &'info [AccountInfo<'info>],
        action: Option<&impl HasSwapParams>,
        token_map: &impl TokenMapAccess,
    ) -> Result<SplitAccountsForGlv<'info>> {
        let len = self.num_markets();

        let markets_end = len;
        let market_tokens_end = markets_end + len;

        require_gte!(
            remaining_accounts.len(),
            market_tokens_end,
            CoreError::InvalidArgument
        );

        let markets = &remaining_accounts[0..markets_end];
        let market_tokens = &remaining_accounts[markets_end..market_tokens_end];
        let remaining_accounts = &remaining_accounts[market_tokens_end..];

        let mut tokens_collector = self.tokens_collector(action);

        for idx in 0..len {
            let market = &markets[idx];
            let market_token = &market_tokens[idx];
            let expected_market_token = Pubkey::new_from_array(
                *self
                    .markets
                    .get_entry_by_index(idx)
                    .expect("never out of range")
                    .0,
            );

            require_keys_eq!(
                market_token.key(),
                expected_market_token,
                CoreError::MarketTokenMintMismatched
            );

            {
                let mint = Account::<anchor_spl::token::Mint>::try_from(market_token)?;
                require!(
                    mint.mint_authority == Some(*store).into(),
                    CoreError::StoreMismatched
                );
            }

            {
                let market = AccountLoader::<Market>::try_from(market)?;
                let market = market.load()?;
                let meta = market.validated_meta(store)?;
                require_keys_eq!(
                    meta.market_token_mint,
                    expected_market_token,
                    CoreError::MarketTokenMintMismatched
                );
                tokens_collector.insert_token(&meta.index_token_mint);
            }
        }

        Ok(SplitAccountsForGlv {
            markets,
            market_tokens,
            remaining_accounts,
            tokens: tokens_collector.into_vec(token_map)?,
        })
    }

    pub(crate) fn validate_market_token_balance(
        &self,
        market_token: &Pubkey,
        new_balance: u64,
        market_pool_value: &i128,
        market_token_supply: &u128,
    ) -> Result<()> {
        let config = self
            .markets
            .get(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        config.validate_balance(new_balance, market_pool_value, market_token_supply)
    }

    pub(crate) fn update_market_token_balance(
        &mut self,
        market_token: &Pubkey,
        new_balance: u64,
    ) -> Result<()> {
        let config = self
            .markets
            .get_mut(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        config.update_balance(new_balance);
        Ok(())
    }

    pub(crate) fn validate_shift_interval(&self) -> Result<()> {
        let interval = self.shift_min_interval_secs;
        if interval == 0 {
            Ok(())
        } else {
            let current = Clock::get()?.unix_timestamp;
            let after = self
                .shift_last_executed_at
                .checked_add(interval as i64)
                .ok_or_else(|| error!(CoreError::ValueOverflow))?;
            require_gte!(current, after, CoreError::GlvShiftIntervalNotYetPassed);
            Ok(())
        }
    }

    pub(crate) fn validate_shift_price_impact(
        &self,
        from_market_token_value: u128,
        to_market_token_value: u128,
    ) -> Result<()> {
        use gmsol_model::utils::div_to_factor;

        if from_market_token_value < to_market_token_value {
            Ok(())
        } else {
            let max_factor = self.shift_max_price_impact_factor;
            let diff = from_market_token_value.abs_diff(to_market_token_value);
            let effective_price_impact_factor = div_to_factor::<_, { constants::MARKET_DECIMALS }>(
                &diff,
                &from_market_token_value,
                false,
            )
            .ok_or_else(|| error!(CoreError::Internal))?;
            require_gte!(
                max_factor,
                effective_price_impact_factor,
                CoreError::GlvShiftMaxPriceImpactExceeded
            );
            Ok(())
        }
    }

    pub(crate) fn validate_shift_value(&self, from_market_token_value: u128) -> Result<()> {
        require_gte!(
            from_market_token_value,
            self.shift_min_value,
            CoreError::GlvShiftValueNotLargeEnough
        );
        Ok(())
    }

    pub(crate) fn update_shift_last_executed_ts(&mut self) -> Result<()> {
        let clock = Clock::get()?;
        self.shift_last_executed_at = clock.unix_timestamp;
        Ok(())
    }
}

#[cfg(feature = "utils")]
impl Glv {
    /// Get last shift executed ts.
    pub fn shift_last_executed_at(&self) -> i64 {
        self.shift_last_executed_at
    }

    /// Get min shift interval.
    pub fn shift_min_interval_secs(&self) -> u32 {
        self.shift_min_interval_secs
    }

    /// Get max shift price impact factor.
    pub fn shift_max_price_impact_factor(&self) -> u128 {
        self.shift_max_price_impact_factor
    }

    /// Get min shift vaule.
    pub fn shift_min_value(&self) -> u128 {
        self.shift_min_value
    }

    /// Get min tokens for first deposit.
    pub fn min_tokens_for_first_deposit(&self) -> u64 {
        self.min_tokens_for_first_deposit
    }
}

/// GLV Update Params.
#[derive(AnchorSerialize, AnchorDeserialize, Default)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct UpdateGlvParams {
    /// Minimum amount for the first GLV deposit.
    pub min_tokens_for_first_deposit: Option<u64>,
    /// Minimum shift interval seconds.
    pub shift_min_interval_secs: Option<u32>,
    /// Maximum price impact factor after shift.
    pub shift_max_price_impact_factor: Option<u128>,
    /// Minimum shift value.
    pub shift_min_value: Option<u128>,
}

impl UpdateGlvParams {
    /// Returns whether the update is empty.
    pub fn is_empty(&self) -> bool {
        self.min_tokens_for_first_deposit.is_none()
            && self.shift_min_interval_secs.is_none()
            && self.shift_max_price_impact_factor.is_none()
            && self.shift_min_value.is_none()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        require!(!self.is_empty(), CoreError::InvalidArgument);
        Ok(())
    }
}

/// GLV Market Config Flag.
#[derive(
    num_enum::IntoPrimitive, Clone, Copy, strum::EnumString, strum::Display, PartialEq, Eq, Hash,
)]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(feature = "enum-iter", derive(strum::EnumIter))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[repr(u8)]
pub enum GlvMarketFlag {
    /// Is deposit allowed.
    IsDepositAllowed,
    // CHECK: cannot have more than `MAX_FLAGS` flags.
}

gmsol_utils::flags!(GlvMarketFlag, MAX_FLAGS, u8);

/// Market Config for GLV.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct GlvMarketConfig {
    max_amount: u64,
    flags: GlvMarketFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 7],
    max_value: u128,
    balance: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 8],
}

impl Default for GlvMarketConfig {
    fn default() -> Self {
        use bytemuck::Zeroable;

        Self::zeroed()
    }
}

impl GlvMarketConfig {
    fn validate_balance(
        &self,
        new_balance: u64,
        market_pool_value: &i128,
        market_token_supply: &u128,
    ) -> Result<()> {
        if self.max_amount == 0 && self.max_value == 0 {
            return Ok(());
        }

        if self.max_amount > 0 {
            require_gte!(
                self.max_amount,
                new_balance,
                CoreError::ExceedMaxGlvMarketTokenBalanceAmount
            );
        }

        if self.max_value > 0 {
            if market_pool_value.is_negative() {
                return err!(CoreError::GlvNegativeMarketPoolValue);
            }

            let value = gmsol_model::utils::market_token_amount_to_usd(
                &(new_balance as u128),
                &market_pool_value.unsigned_abs(),
                market_token_supply,
            )
            .ok_or_else(|| error!(CoreError::FailedToCalculateGlvValueForMarket))?;
            require_gte!(
                self.max_value,
                value,
                CoreError::ExceedMaxGlvMarketTokenBalanceValue
            );
        }

        Ok(())
    }

    fn update_balance(&mut self, new_balance: u64) {
        self.balance = new_balance;
    }

    /// Get balance.
    pub fn balance(&self) -> u64 {
        self.balance
    }

    pub(crate) fn toggle_flag(&mut self, flag: GlvMarketFlag, enable: bool) -> Result<bool> {
        let current = self.flags.get_flag(flag);
        require_neq!(current, enable, CoreError::PreconditionsAreNotMet);
        Ok(self.flags.set_flag(flag, enable))
    }

    /// Get flag.
    pub fn get_flag(&self, flag: GlvMarketFlag) -> bool {
        self.flags.get_flag(flag)
    }
}

#[cfg(feature = "utils")]
impl GlvMarketConfig {
    /// Get max amount.
    pub fn max_amount(&self) -> u64 {
        self.max_amount
    }

    /// Get max value.
    pub fn max_value(&self) -> u128 {
        self.max_value
    }
}

pub(crate) struct SplitAccountsForGlv<'info> {
    pub(crate) markets: &'info [AccountInfo<'info>],
    pub(crate) market_tokens: &'info [AccountInfo<'info>],
    pub(crate) remaining_accounts: &'info [AccountInfo<'info>],
    pub(crate) tokens: Vec<Pubkey>,
}

/// Glv Deposit.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvDeposit {
    /// Header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: GlvDepositTokenAccounts,
    /// Params.
    pub(crate) params: GlvDepositParams,
    /// Swap params.
    pub(crate) swap: SwapParams,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 4],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl Action for GlvDeposit {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

impl Closable for GlvDeposit {
    type ClosedEvent = GlvDepositRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        GlvDepositRemoved::new(
            self.header.id(),
            *self.header.store(),
            *address,
            self.tokens().market_token(),
            self.tokens().glv_token(),
            *self.header.owner(),
            self.header.action_state()?,
            reason,
        )
    }
}

impl Seed for GlvDeposit {
    const SEED: &'static [u8] = b"glv_deposit";
}

impl gmsol_utils::InitSpace for GlvDeposit {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl GlvDeposit {
    /// Validate the GLV deposit before execution.
    ///
    /// # CHECK
    /// - This deposit must have been initialized.
    /// - The `glv` and `glv_token` must match.
    /// - The `market_token` must be a valid token account.
    /// - The `glv_token` must be a valid token account.
    ///
    /// # Errors
    /// - The address of `market_token` must match the market token address of this deposit.
    /// - The address of `glv_token` must match the glv token address of this deposit.
    pub(crate) fn unchecked_validate_for_execution(
        &self,
        glv_token: &InterfaceAccount<token_interface::Mint>,
        glv: &Glv,
    ) -> Result<()> {
        require_keys_eq!(
            glv_token.key(),
            self.tokens.glv_token(),
            CoreError::TokenMintMismatched,
        );

        let supply = glv_token.supply;

        if supply == 0 {
            Self::validate_first_deposit(
                &self.header().receiver(),
                self.params.min_glv_token_amount,
                glv,
            )?;
        }

        Ok(())
    }

    pub(crate) fn is_market_deposit_required(&self) -> bool {
        self.params.deposit.initial_long_token_amount != 0
            || self.params.deposit.initial_short_token_amount != 0
    }

    /// Get first deposit receiver.
    #[inline]
    pub fn first_deposit_receiver() -> Pubkey {
        Deposit::first_deposit_receiver()
    }

    fn validate_first_deposit(receiver: &Pubkey, min_amount: u64, glv: &Glv) -> Result<()> {
        let min_tokens_for_first_deposit = glv.min_tokens_for_first_deposit;

        // Skip first deposit check if the amount is zero.
        if min_tokens_for_first_deposit == 0 {
            return Ok(());
        }

        require_keys_eq!(
            *receiver,
            Self::first_deposit_receiver(),
            CoreError::InvalidReceiverForFirstDeposit
        );

        require_gte!(
            min_amount,
            min_tokens_for_first_deposit,
            CoreError::NotEnoughGlvTokenAmountForFirstDeposit,
        );

        Ok(())
    }

    pub(crate) fn validate_output_amount(&self, amount: u64) -> Result<()> {
        require_gte!(
            amount,
            self.params.min_glv_token_amount,
            CoreError::InsufficientOutputAmount
        );

        Ok(())
    }

    /// Get token infos.
    pub fn tokens(&self) -> &GlvDepositTokenAccounts {
        &self.tokens
    }
}

impl HasSwapParams for GlvDeposit {
    fn swap(&self) -> &SwapParams {
        &self.swap
    }
}

/// Token and accounts.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvDepositTokenAccounts {
    /// Initial long token and account.
    pub initial_long_token: TokenAndAccount,
    /// Initial short token and account.
    pub initial_short_token: TokenAndAccount,
    /// Market token and account.
    pub(crate) market_token: TokenAndAccount,
    /// GLV token and account.
    pub(crate) glv_token: TokenAndAccount,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl GlvDepositTokenAccounts {
    /// Get market token.
    pub fn market_token(&self) -> Pubkey {
        self.market_token
            .token()
            .expect("uninitialized GLV Deposit account")
    }

    /// Get market token account.
    pub fn market_token_account(&self) -> Pubkey {
        self.market_token
            .account()
            .expect("uninitalized GLV Deposit account")
    }

    /// Get GLV token.
    pub fn glv_token(&self) -> Pubkey {
        self.glv_token
            .token()
            .expect("uninitialized GLV Deposit account")
    }

    /// Get GLV token account.
    pub fn glv_token_account(&self) -> Pubkey {
        self.glv_token
            .account()
            .expect("uninitalized GLV Deposit account")
    }
}

/// GLV Deposit Params.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvDepositParams {
    /// Deposit params.
    pub(crate) deposit: DepositParams,
    /// The amount of market tokens to deposit.
    pub(crate) market_token_amount: u64,
    /// The minimum acceptable amount of glv tokens to receive.
    pub(crate) min_glv_token_amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

/// Glv Withdrawal.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvWithdrawal {
    /// Header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: GlvWithdrawalTokenAccounts,
    /// Params.
    pub(crate) params: GlvWithdrawalParams,
    /// Swap params.
    pub(crate) swap: SwapParams,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 4],
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl GlvWithdrawal {
    /// Get tokens.
    pub fn tokens(&self) -> &GlvWithdrawalTokenAccounts {
        &self.tokens
    }

    /// Get swap params.
    pub fn swap(&self) -> &SwapParams {
        &self.swap
    }
}

impl Action for GlvWithdrawal {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

impl Closable for GlvWithdrawal {
    type ClosedEvent = GlvWithdrawalRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        GlvWithdrawalRemoved::new(
            self.header.id,
            self.header.store,
            *address,
            self.tokens.market_token(),
            self.tokens.glv_token(),
            self.header.owner,
            self.header.action_state()?,
            reason,
        )
    }
}

impl Seed for GlvWithdrawal {
    const SEED: &'static [u8] = b"glv_withdrawal";
}

impl gmsol_utils::InitSpace for GlvWithdrawal {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl HasSwapParams for GlvWithdrawal {
    fn swap(&self) -> &SwapParams {
        &self.swap
    }
}

/// Token and accounts.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvWithdrawalTokenAccounts {
    /// Final ong token and account.
    pub(crate) final_long_token: TokenAndAccount,
    /// Final short token and account.
    pub(crate) final_short_token: TokenAndAccount,
    /// Market token and account.
    pub(crate) market_token: TokenAndAccount,
    /// GLV token and account.
    pub(crate) glv_token: TokenAndAccount,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl GlvWithdrawalTokenAccounts {
    /// Get market token.
    pub fn market_token(&self) -> Pubkey {
        self.market_token
            .token()
            .expect("uninitialized GLV Withdrawal account")
    }

    /// Get market token account.
    pub fn market_token_account(&self) -> Pubkey {
        self.market_token
            .account()
            .expect("uninitalized GLV Withdrawal account")
    }

    /// Get GLV token.
    pub fn glv_token(&self) -> Pubkey {
        self.glv_token
            .token()
            .expect("uninitialized GLV Withdrawal account")
    }

    /// Get GLV token account.
    pub fn glv_token_account(&self) -> Pubkey {
        self.glv_token
            .account()
            .expect("uninitalized GLV Withdrawal account")
    }

    /// Get final long token.
    pub fn final_long_token(&self) -> Pubkey {
        self.final_long_token
            .token()
            .expect("uninitialized GLV Withdrawal account")
    }

    /// Get final long token account.
    pub fn final_long_token_account(&self) -> Pubkey {
        self.final_long_token
            .account()
            .expect("uninitalized GLV Withdrawal account")
    }

    /// Get final short token.
    pub fn final_short_token(&self) -> Pubkey {
        self.final_short_token
            .token()
            .expect("uninitialized GLV Withdrawal account")
    }

    /// Get final short token account.
    pub fn final_short_token_account(&self) -> Pubkey {
        self.final_short_token
            .account()
            .expect("uninitalized GLV Withdrawal account")
    }
}

/// GLV Withdrawal Params.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvWithdrawalParams {
    /// The amount of GLV tokens to burn.
    pub(crate) glv_token_amount: u64,
    /// The minimum acceptable amount of final long tokens to receive.
    pub min_final_long_token_amount: u64,
    /// The minimum acceptable amount of final short tokens to receive.
    pub min_final_short_token_amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

/// Glv Shift.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlvShift {
    pub(crate) shift: Shift,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
}

impl Action for GlvShift {
    const MIN_EXECUTION_LAMPORTS: u64 = 0;

    fn header(&self) -> &ActionHeader {
        &self.shift.header
    }
}

impl Closable for GlvShift {
    type ClosedEvent = ShiftRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        let header = self.header();
        let tokens = self.tokens();
        ShiftRemoved::new(
            header.id,
            header.store,
            *address,
            tokens.from_market_token(),
            header.owner,
            header.action_state()?,
            reason,
        )
    }
}

impl Seed for GlvShift {
    const SEED: &'static [u8] = Shift::SEED;
}

impl gmsol_utils::InitSpace for GlvShift {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl GlvShift {
    /// Get the GLV address.
    pub fn glv(&self) -> &Pubkey {
        &self.shift.header.owner
    }

    /// Get token infos.
    pub fn tokens(&self) -> &shift::TokenAccounts {
        self.shift.tokens()
    }

    pub(crate) fn header_mut(&mut self) -> &mut ActionHeader {
        &mut self.shift.header
    }

    /// Get the funder.
    pub fn funder(&self) -> &Pubkey {
        self.shift.header().rent_receiver()
    }
}

impl Borrow<Shift> for GlvShift {
    fn borrow(&self) -> &Shift {
        &self.shift
    }
}

impl BorrowMut<Shift> for GlvShift {
    fn borrow_mut(&mut self) -> &mut Shift {
        &mut self.shift
    }
}
