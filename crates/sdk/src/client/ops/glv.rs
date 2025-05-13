use std::{collections::BTreeSet, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::UpdateGlvParams,
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::glv::GlvMarketFlag;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer, system_program};

/// GLV operations.
pub trait GlvOps<C> {
    /// Initialize GLV.
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u16,
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
}

impl<C: Deref<Target = impl Signer> + Clone> GlvOps<C> for crate::Client<C> {
    fn initialize_glv(
        &self,
        store: &Pubkey,
        index: u16,
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
            true,
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
            .anchor_args(args::InitializeGlv {
                index,
                length: length
                    .try_into()
                    .map_err(|_| crate::Error::unknown("too many markets"))?,
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
            .anchor_args(args::UpdateGlvMarketConfig {
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
            .anchor_args(args::ToggleGlvMarketFlag {
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
            .anchor_args(args::UpdateGlvConfig { params })
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
            .anchor_args(args::InsertGlvMarket {})
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
        let store_wallet = self.find_store_wallet_address(store);
        let store_wallet_ata = get_associated_token_address_with_program_id(
            &store_wallet,
            market_token,
            token_program_id,
        );
        self.store_transaction()
            .anchor_accounts(accounts::RemoveGlvMarket {
                authority: self.payer(),
                store: *store,
                store_wallet,
                glv,
                market_token: *market_token,
                vault,
                store_wallet_ata,
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            })
            .anchor_args(args::RemoveGlvMarket {})
    }
}

pub(crate) fn split_to_accounts(
    market_tokens: impl IntoIterator<Item = Pubkey>,
    glv: &Pubkey,
    store: &Pubkey,
    store_program_id: &Pubkey,
    token_program_id: &Pubkey,
    with_vaults: bool,
) -> (Vec<AccountMeta>, usize) {
    let market_token_addresses = market_tokens.into_iter().collect::<BTreeSet<_>>();

    let markets = market_token_addresses.iter().map(|token| {
        AccountMeta::new_readonly(
            crate::pda::find_market_address(store, token, store_program_id).0,
            false,
        )
    });

    let market_tokens = market_token_addresses
        .iter()
        .map(|token| AccountMeta::new_readonly(*token, false));

    let length = market_token_addresses.len();

    let accounts = if with_vaults {
        let market_token_vaults = market_token_addresses.iter().map(|token| {
            let market_token_vault =
                get_associated_token_address_with_program_id(glv, token, token_program_id);

            AccountMeta::new(market_token_vault, false)
        });

        markets
            .chain(market_tokens)
            .chain(market_token_vaults)
            .collect::<Vec<_>>()
    } else {
        markets.chain(market_tokens).collect::<Vec<_>>()
    };

    (accounts, length)
}
