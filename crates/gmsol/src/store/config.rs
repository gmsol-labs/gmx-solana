use std::ops::Deref;

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{
    accounts, instruction,
    states::{AddressKey, Amount, AmountKey, Factor, FactorKey},
};

/// Config Operations.
pub trait ConfigOps<C> {
    /// Insert a global amount.
    fn insert_global_amount(
        &self,
        store: &Pubkey,
        key: &str,
        amount: &Amount,
    ) -> TransactionBuilder<C>;

    /// Insert a global factor.
    fn insert_global_factor(
        &self,
        store: &Pubkey,
        key: &str,
        factor: &Factor,
    ) -> TransactionBuilder<C>;

    /// Insert a global address.
    fn insert_global_address(
        &self,
        store: &Pubkey,
        key: &str,
        address: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Insert a global amount by key.
    fn insert_global_amount_by_key(
        &self,
        store: &Pubkey,
        key: AmountKey,
        amount: &Amount,
    ) -> TransactionBuilder<C> {
        let key = key.to_string();
        self.insert_global_amount(store, &key, amount)
    }

    /// Insert a global factor by key.
    fn insert_global_factor_by_key(
        &self,
        store: &Pubkey,
        key: FactorKey,
        factor: &Factor,
    ) -> TransactionBuilder<C> {
        let key = key.to_string();
        self.insert_global_factor(store, &key, factor)
    }

    /// Insert a global address by key.
    fn insert_global_address_by_key(
        &self,
        store: &Pubkey,
        key: AddressKey,
        address: &Pubkey,
    ) -> TransactionBuilder<C> {
        let key = key.to_string();
        self.insert_global_address(store, &key, address)
    }
}

impl<C, S> ConfigOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn insert_global_amount(
        &self,
        store: &Pubkey,
        key: &str,
        amount: &Amount,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(instruction::InsertAmount {
                key: key.to_string(),
                amount: *amount,
            })
            .anchor_accounts(accounts::InsertConfig {
                authority,
                store: *store,
            })
    }

    fn insert_global_factor(
        &self,
        store: &Pubkey,
        key: &str,
        factor: &Factor,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(instruction::InsertFactor {
                key: key.to_string(),
                factor: *factor,
            })
            .anchor_accounts(accounts::InsertConfig {
                authority,
                store: *store,
            })
    }

    fn insert_global_address(
        &self,
        store: &Pubkey,
        key: &str,
        address: &Pubkey,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(instruction::InsertAddress {
                key: key.to_string(),
                address: *address,
            })
            .anchor_accounts(accounts::InsertConfig {
                authority,
                store: *store,
            })
    }
}
