use std::ops::Deref;

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{
    accounts, instruction,
    states::{Factor, FactorKey},
};

/// Data Store management for GMSOL.
pub trait StoreOps<C> {
    /// Initialize [`Store`](gmsol_store::states::Store) account.
    fn initialize_store<S: Signer + 'static>(
        &self,
        key: &str,
        authority: Option<S>,
        receiver: Option<S>,
        holding: Option<S>,
    ) -> TransactionBuilder<C>;

    /// Transfer store authority.
    fn transfer_store_authority(
        &self,
        store: &Pubkey,
        new_authority: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Accept store authority.
    fn accept_store_authority(&self, store: &Pubkey) -> TransactionBuilder<C>;

    /// Transfer receiver.
    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> TransactionBuilder<C>;

    /// Set new token map.
    fn set_token_map(&self, store: &Pubkey, token_map: &Pubkey) -> TransactionBuilder<C>;

    /// Insert factor.
    fn insert_factor(
        &self,
        store: &Pubkey,
        key: FactorKey,
        factor: Factor,
    ) -> TransactionBuilder<C>;
}

impl<C, S> StoreOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn initialize_store<S2: Signer + 'static>(
        &self,
        key: &str,
        authority: Option<S2>,
        receiver: Option<S2>,
        holding: Option<S2>,
    ) -> TransactionBuilder<C> {
        let store = self.find_store_address(key);
        let authority_address = authority.as_ref().map(|s| s.pubkey());
        let receiver_address = receiver.as_ref().map(|s| s.pubkey());
        let holding_address = holding.as_ref().map(|s| s.pubkey());
        let mut rpc = self
            .store_transaction()
            .anchor_accounts(accounts::Initialize {
                payer: self.payer(),
                authority: authority_address,
                receiver: receiver_address,
                holding: holding_address,
                store,
                system_program: system_program::ID,
            })
            .anchor_args(instruction::Initialize {
                key: key.to_string(),
            });

        for signer in authority.into_iter().chain(receiver).chain(holding) {
            rpc = rpc.owned_signer(signer);
        }

        rpc
    }

    fn transfer_store_authority(
        &self,
        store: &Pubkey,
        next_authority: &Pubkey,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TransferStoreAuthority {})
            .anchor_accounts(accounts::TransferStoreAuthority {
                authority: self.payer(),
                store: *store,
                next_authority: *next_authority,
            })
    }

    fn accept_store_authority(&self, store: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::AcceptStoreAuthority {})
            .anchor_accounts(accounts::AcceptStoreAuthority {
                next_authority: self.payer(),
                store: *store,
            })
    }

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::TransferReceiver {})
            .anchor_accounts(accounts::TransferReceiver {
                authority: self.payer(),
                store: *store,
                next_receiver: *new_receiver,
            })
    }

    fn set_token_map(&self, store: &Pubkey, token_map: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::SetTokenMap {})
            .anchor_accounts(accounts::SetTokenMap {
                authority: self.payer(),
                store: *store,
                token_map: *token_map,
            })
    }

    fn insert_factor(
        &self,
        store: &Pubkey,
        key: FactorKey,
        factor: Factor,
    ) -> TransactionBuilder<C> {
        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::InsertConfig {
                authority: self.payer(),
                store: *store,
            });
        match key {
            FactorKey::OrderFeeDiscountForReferredUser => {
                rpc.anchor_args(instruction::InsertOrderFeeDiscountForReferredUser { factor })
            }
            _ => rpc.anchor_args(instruction::InsertFactor {
                key: key.to_string(),
                factor,
            }),
        }
    }
}
