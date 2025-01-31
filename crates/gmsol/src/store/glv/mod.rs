use std::{collections::BTreeSet, ops::Deref};

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{
    accounts, instruction,
    states::{
        glv::{GlvMarketFlag, UpdateGlvParams},
        Market,
    },
};

mod deposit;
mod shift;
mod withdrawal;

pub use self::{
    deposit::{
        CloseGlvDepositBuilder, CloseGlvDepositHint, CreateGlvDepositBuilder, CreateGlvDepositHint,
        ExecuteGlvDepositBuilder, ExecuteGlvDepositHint,
    },
    shift::{
        CloseGlvShiftBuilder, CloseGlvShiftHint, CreateGlvShiftBuilder, ExecuteGlvShiftBuilder,
        ExecuteGlvShiftHint,
    },
    withdrawal::{
        CloseGlvWithdrawalBuilder, CloseGlvWithdrawalHint, CreateGlvWithdrawalBuilder,
        CreateGlvWithdrawalHint, ExecuteGlvWithdrawalBuilder, ExecuteGlvWithdrawalHint,
    },
};

/// Glv Operations.
pub trait GlvOps<C> {
    /// Initialize GLV.
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u8,
        market_tokens: impl IntoIterator<Item = Pubkey>,
    ) -> crate::Result<(TransactionBuilder<C>, Pubkey)>;

    /// GLV Update Market Config.
    fn update_glv_market_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        max_amount: Option<u64>,
        max_value: Option<u128>,
    ) -> TransactionBuilder<C>;

    /// GLV toggle market flag.
    fn toggle_glv_market_flag(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        flag: GlvMarketFlag,
        enable: bool,
    ) -> TransactionBuilder<C>;

    /// Update GLV config.
    fn update_glv_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        params: UpdateGlvParams,
    ) -> TransactionBuilder<C>;

    /// Insert GLV market.
    fn insert_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;

    /// Remove GLV market.
    fn remove_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;

    /// Create a GLV deposit.
    fn create_glv_deposit(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateGlvDepositBuilder<C>;

    /// Close a GLV deposit.
    fn close_glv_deposit(&self, glv_deposit: &Pubkey) -> CloseGlvDepositBuilder<C>;

    /// Execute the given GLV deposit.
    fn execute_glv_deposit(
        &self,
        oracle: &Pubkey,
        glv_deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvDepositBuilder<C>;

    fn create_glv_withdrawal(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvWithdrawalBuilder<C>;

    /// Close a GLV withdrawal.
    fn close_glv_withdrawal(&self, glv_withdrawal: &Pubkey) -> CloseGlvWithdrawalBuilder<C>;

    /// Execute the given GLV deposit.
    fn execute_glv_withdrawal(
        &self,
        oracle: &Pubkey,
        glv_withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvWithdrawalBuilder<C>;

    fn create_glv_shift(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvShiftBuilder<C>;

    fn close_glv_shift(&self, glv_shift: &Pubkey) -> CloseGlvShiftBuilder<C>;

    fn execute_glv_shift(
        &self,
        oracle: &Pubkey,
        glv_shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvShiftBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> GlvOps<C> for crate::Client<C> {
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u8,
        market_tokens: impl IntoIterator<Item = Pubkey>,
    ) -> crate::Result<(TransactionBuilder<C>, Pubkey)> {
        let authority = self.payer();
        let glv_token = self.find_glv_token_address(store, index);
        let glv = self.find_glv_address(&glv_token);
        let market_token_program_id = anchor_spl::token::ID;

        let (accounts, length) = split_to_accounts(
            market_tokens,
            &glv,
            store,
            self.store_program_id(),
            &market_token_program_id,
        );

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeGlv {
                authority,
                store: *store,
                glv_token,
                glv,
                system_program: system_program::ID,
                token_program: anchor_spl::token_2022::ID,
                market_token_program: market_token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(instruction::InitializeGlv {
                index,
                length: length
                    .try_into()
                    .map_err(|_| crate::Error::invalid_argument("too many markets"))?,
            })
            .accounts(accounts);
        Ok((rpc, glv_token))
    }

    fn update_glv_market_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        max_amount: Option<u64>,
        max_value: Option<u128>,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvMarketConfig {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
            })
            .anchor_args(instruction::UpdateGlvMarketConfig {
                max_amount,
                max_value,
            })
    }

    fn toggle_glv_market_flag(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        flag: GlvMarketFlag,
        enable: bool,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvMarketConfig {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
            })
            .anchor_args(instruction::ToggleGlvMarketFlag {
                flag: flag.to_string(),
                enable,
            })
    }

    fn update_glv_config(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        params: UpdateGlvParams,
    ) -> TransactionBuilder<C> {
        let glv = self.find_glv_address(glv_token);
        self.store_transaction()
            .anchor_accounts(accounts::UpdateGlvConfig {
                authority: self.payer(),
                store: *store,
                glv,
            })
            .anchor_args(instruction::UpdateGlvConfig { params })
    }

    fn insert_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);
        let glv = self.find_glv_address(glv_token);
        let market = self.find_market_address(store, market_token);
        let vault =
            get_associated_token_address_with_program_id(&glv, market_token, token_program_id);
        self.store_transaction()
            .anchor_accounts(accounts::InsertGlvMarket {
                authority: self.payer(),
                store: *store,
                glv,
                market_token: *market_token,
                market,
                vault,
                system_program: system_program::ID,
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(instruction::InsertGlvMarket {})
    }

    fn remove_glv_market(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        token_program_id: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        let token_program_id = token_program_id.unwrap_or(&anchor_spl::token::ID);
        let glv = self.find_glv_address(glv_token);
        let vault =
            get_associated_token_address_with_program_id(&glv, market_token, token_program_id);
        self.store_transaction()
            .anchor_accounts(accounts::RemoveGlvMarket {
                authority: self.payer(),
                store: *store,
                store_wallet: self.find_store_wallet_address(store),
                glv,
                market_token: *market_token,
                vault,
                token_program: *token_program_id,
            })
            .anchor_args(instruction::RemoveGlvMarket {})
    }

    fn create_glv_deposit(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateGlvDepositBuilder<C> {
        CreateGlvDepositBuilder::new(self, *store, *glv_token, *market_token)
    }

    fn close_glv_deposit(&self, glv_deposit: &Pubkey) -> CloseGlvDepositBuilder<C> {
        CloseGlvDepositBuilder::new(self, *glv_deposit)
    }

    fn execute_glv_deposit(
        &self,
        oracle: &Pubkey,
        glv_deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvDepositBuilder<C> {
        ExecuteGlvDepositBuilder::new(self, *oracle, *glv_deposit, cancel_on_execution_error)
    }

    fn create_glv_withdrawal(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvWithdrawalBuilder<C> {
        CreateGlvWithdrawalBuilder::new(self, *store, *glv_token, *market_token, amount)
    }

    fn close_glv_withdrawal(&self, glv_withdrawal: &Pubkey) -> CloseGlvWithdrawalBuilder<C> {
        CloseGlvWithdrawalBuilder::new(self, *glv_withdrawal)
    }

    fn execute_glv_withdrawal(
        &self,
        oracle: &Pubkey,
        glv_withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvWithdrawalBuilder<C> {
        ExecuteGlvWithdrawalBuilder::new(self, *oracle, *glv_withdrawal, cancel_on_execution_error)
    }

    fn create_glv_shift(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvShiftBuilder<C> {
        CreateGlvShiftBuilder::new(
            self,
            store,
            glv_token,
            from_market_token,
            to_market_token,
            amount,
        )
    }

    fn close_glv_shift(&self, glv_shift: &Pubkey) -> CloseGlvShiftBuilder<C> {
        CloseGlvShiftBuilder::new(self, glv_shift)
    }

    fn execute_glv_shift(
        &self,
        oracle: &Pubkey,
        glv_shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvShiftBuilder<C> {
        let mut builder = ExecuteGlvShiftBuilder::new(self, oracle, glv_shift);
        builder.cancel_on_execution_error(cancel_on_execution_error);
        builder
    }
}

fn split_to_accounts(
    market_tokens: impl IntoIterator<Item = Pubkey>,
    glv: &Pubkey,
    store: &Pubkey,
    store_program_id: &Pubkey,
    token_program_id: &Pubkey,
) -> (Vec<AccountMeta>, usize) {
    let market_token_addresses = market_tokens.into_iter().collect::<BTreeSet<_>>();

    let markets = market_token_addresses.iter().map(|token| {
        AccountMeta::new_readonly(
            Market::find_market_address(store, token, store_program_id).0,
            false,
        )
    });

    let market_tokens = market_token_addresses
        .iter()
        .map(|token| AccountMeta::new_readonly(*token, false));

    let market_token_vaults = market_token_addresses.iter().map(|token| {
        let market_token_vault =
            get_associated_token_address_with_program_id(glv, token, token_program_id);

        AccountMeta::new(market_token_vault, false)
    });

    let length = market_token_addresses.len();
    let accounts = markets
        .chain(market_tokens)
        .chain(market_token_vaults)
        .collect::<Vec<_>>();

    (accounts, length)
}

impl<C: Deref<Target = impl Signer> + Clone> crate::Client<C> {
    /// Create first GLV deposit.
    pub fn create_first_glv_deposit(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateGlvDepositBuilder<C> {
        let mut builder = self.create_glv_deposit(store, glv_token, market_token);
        builder.receiver(Some(self.find_first_deposit_owner_address()));
        builder
    }
}
