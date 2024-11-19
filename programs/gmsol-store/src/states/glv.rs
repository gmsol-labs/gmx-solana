use std::{
    borrow::{Borrow, BorrowMut},
    collections::BTreeSet,
};

use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::{
    events::{GlvDepositRemoved, GlvWithdrawalRemoved, ShiftRemoved},
    states::{Deposit, Market},
    utils::token::validate_associated_token_account,
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

const MAX_ALLOWED_NUMBER_OF_MARKETS: usize = 128;

/// Glv.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Glv {
    version: u8,
    /// Bump seed.
    pub(crate) bump: u8,
    bump_bytes: [u8; 1],
    /// Index.
    pub(crate) index: u8,
    padding: [u8; 4],
    pub(crate) store: Pubkey,
    pub(crate) glv_token: Pubkey,
    pub(crate) long_token: Pubkey,
    pub(crate) short_token: Pubkey,
    pub(crate) min_tokens_for_first_deposit: u64,
    reserve: [u8; 256],
    /// Market config map with market token addresses as keys.
    markets: GlvMarkets,
}

/// Market Config for GLV.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GlvMarketConfig {
    max_amount: u64,
    padding: [u8; 8],
    max_value: u128,
}

impl Default for GlvMarketConfig {
    fn default() -> Self {
        use bytemuck::Zeroable;

        Self::zeroed()
    }
}

impl GlvMarketConfig {
    fn validate_market_token_balance(
        &self,
        new_balance: u64,
        market_pool_value: &u128,
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
            let value = gmsol_model::utils::market_token_amount_to_usd(
                &(new_balance as u128),
                market_pool_value,
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
    pub fn find_glv_token_pda(store: &Pubkey, index: u8, program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[Self::GLV_TOKEN_SEED, store.as_ref(), &[index]],
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
        index: u8,
        store: &Pubkey,
        glv_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        market_tokens: &BTreeSet<Pubkey>,
    ) -> Result<()> {
        let expected_glv_token = Self::find_glv_token_pda(store, index, &crate::ID).0;
        require_eq!(expected_glv_token, *glv_token, CoreError::InvalidArgument);

        self.version = 0;
        self.bump = bump;
        self.bump_bytes = [bump];
        self.index = index;
        self.store = *store;
        self.glv_token = *glv_token;
        self.long_token = *long_token;
        self.short_token = *short_token;

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
                    require_eq!(
                        *long_token,
                        meta.long_token_mint,
                        CoreError::TokenMintMismatched
                    );
                    require_eq!(
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
    pub fn index(&self) -> u8 {
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

    /// Create a new [`TokensCollector`].
    pub fn tokens_collector(&self, action: Option<&impl HasSwapParams>) -> TokensCollector {
        TokensCollector::new(action, self.num_markets())
    }

    /// Split remaining accounts.
    pub(crate) fn validate_and_split_remaining_accounts<'info>(
        &self,
        address: &Pubkey,
        store: &Pubkey,
        token_program_id: &Pubkey,
        remaining_accounts: &'info [AccountInfo<'info>],
        action: Option<&impl HasSwapParams>,
        token_map: &impl TokenMapAccess,
    ) -> Result<SplitAccountsForGlv<'info>> {
        let len = self.num_markets();

        let markets_end = len;
        let market_tokens_end = markets_end + len;
        let market_token_vaults_end = market_tokens_end + len;

        require_gte!(
            remaining_accounts.len(),
            market_token_vaults_end,
            CoreError::InvalidArgument
        );

        let markets = &remaining_accounts[0..markets_end];
        let market_tokens = &remaining_accounts[markets_end..market_tokens_end];
        let market_token_vaults = &remaining_accounts[market_tokens_end..market_token_vaults_end];
        let remaining_accounts = &remaining_accounts[market_token_vaults_end..];

        let mut tokens_collector = self.tokens_collector(action);

        for idx in 0..len {
            let market = &markets[idx];
            let market_token = &market_tokens[idx];
            let market_token_vault = &market_token_vaults[idx];
            let expected_market_token = Pubkey::new_from_array(
                *self
                    .markets
                    .get_entry_by_index(idx)
                    .expect("never out of range")
                    .0,
            );

            require_eq!(
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
                require_eq!(
                    meta.market_token_mint,
                    expected_market_token,
                    CoreError::MarketTokenMintMismatched
                );
                tokens_collector.insert_token(&meta.index_token_mint);
            }

            validate_associated_token_account(
                market_token_vault,
                address,
                market_token.key,
                token_program_id,
            )?;
        }

        Ok(SplitAccountsForGlv {
            markets,
            market_tokens,
            market_token_vaults,
            remaining_accounts,
            tokens: tokens_collector.into_vec(token_map)?,
        })
    }

    pub(crate) fn validate_market_token_balance(
        &self,
        market_token: &Pubkey,
        new_balance: u64,
        market_pool_value: &u128,
        market_token_supply: &u128,
    ) -> Result<()> {
        let config = self
            .markets
            .get(market_token)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        config.validate_market_token_balance(new_balance, market_pool_value, market_token_supply)
    }
}

pub(crate) struct SplitAccountsForGlv<'info> {
    pub(crate) markets: &'info [AccountInfo<'info>],
    pub(crate) market_tokens: &'info [AccountInfo<'info>],
    pub(crate) market_token_vaults: &'info [AccountInfo<'info>],
    pub(crate) remaining_accounts: &'info [AccountInfo<'info>],
    pub(crate) tokens: Vec<Pubkey>,
}

/// Glv Deposit.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvDeposit {
    /// Header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: GlvDepositTokenAccounts,
    /// Params.
    pub(crate) params: GlvDepositParams,
    /// Swap params.
    pub(crate) swap: SwapParams,
    padding_1: [u8; 4],
    reserve: [u8; 128],
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
    /// - The `market` and `market_token` must match.
    /// - The `glv` and `glv_token` must match.
    /// - The `market_token` must be a valid token account.
    /// - The `glv_token` must be a valid token account.
    ///
    /// # Errors
    /// - The address of `market_token` must match the market token address of this deposit.
    /// - The address of `glv_token` must match the glv token address of this deposit.
    pub(crate) fn unchecked_validate_for_execution(
        &self,
        market_token: &AccountInfo,
        market: &Market,
        glv_token: &AccountInfo,
        glv: &Glv,
    ) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        require_eq!(
            *market_token.key,
            self.tokens.market_token(),
            CoreError::MarketTokenMintMismatched
        );

        let supply = amount(market_token)?;

        if supply == 0 && self.is_market_deposit_required() {
            Deposit::validate_first_deposit(
                &self.header.owner,
                self.params.deposit.min_market_token_amount,
                market,
            )?;
        }

        require_eq!(
            *glv_token.key,
            self.tokens.glv_token(),
            CoreError::TokenMintMismatched,
        );

        let supply = amount(glv_token)?;

        if supply == 0 {
            Self::validate_first_deposit(
                &self.header.owner,
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

    #[inline]
    fn first_deposit_owner() -> Pubkey {
        Deposit::first_deposit_owner()
    }

    fn validate_first_deposit(owner: &Pubkey, min_amount: u64, glv: &Glv) -> Result<()> {
        let min_tokens_for_first_deposit = glv.min_tokens_for_first_deposit;

        // Skip first deposit check if the amount is zero.
        if min_tokens_for_first_deposit == 0 {
            return Ok(());
        }

        require_eq!(
            *owner,
            Self::first_deposit_owner(),
            CoreError::InvalidOwnerForFirstDeposit
        );

        require_gte!(
            min_amount,
            min_tokens_for_first_deposit,
            CoreError::NotEnoughGlvTokenAmountForFirstDeposit,
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
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvDepositTokenAccounts {
    /// Initial long token and account.
    pub initial_long_token: TokenAndAccount,
    /// Initial short token and account.
    pub initial_short_token: TokenAndAccount,
    /// Market token and account.
    pub(crate) market_token: TokenAndAccount,
    /// GLV token and account.
    pub(crate) glv_token: TokenAndAccount,
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
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvDepositParams {
    /// Deposit params.
    pub(crate) deposit: DepositParams,
    /// The amount of market tokens to deposit.
    pub(crate) market_token_amount: u64,
    /// The minimum acceptable amount of glv tokens to receive.
    pub(crate) min_glv_token_amount: u64,
    reserved: [u8; 64],
}

/// Glv Withdrawal.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvWithdrawal {
    /// Header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: GlvWithdrawalTokenAccounts,
    /// Params.
    pub(crate) params: GlvWithdrawalParams,
    /// Swap params.
    pub(crate) swap: SwapParams,
    padding_1: [u8; 4],
    reserve: [u8; 128],
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
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvWithdrawalTokenAccounts {
    /// Final ong token and account.
    pub(crate) final_long_token: TokenAndAccount,
    /// Final short token and account.
    pub(crate) final_short_token: TokenAndAccount,
    /// Market token and account.
    pub(crate) market_token: TokenAndAccount,
    /// GLV token and account.
    pub(crate) glv_token: TokenAndAccount,
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
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvWithdrawalParams {
    /// The amount of GLV tokens to burn.
    pub(crate) glv_token_amount: u64,
    /// The minimum acceptable amount of final long tokens to receive.
    pub min_final_long_token_amount: u64,
    /// The minimum acceptable amount of final short tokens to receive.
    pub min_final_short_token_amount: u64,
    reserved: [u8; 64],
}

/// Glv Shift.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct GlvShift {
    pub(crate) shift: Shift,
    pub(crate) funder: Pubkey,
}

impl Action for GlvShift {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

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
        &self.funder
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
