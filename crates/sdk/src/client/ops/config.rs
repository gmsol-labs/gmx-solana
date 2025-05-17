use std::ops::Deref;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::config::{
    ActionDisabledFlag, AddressKey, AmountKey, DomainDisabledFlag, FactorKey,
};
use solana_sdk::{pubkey::Pubkey, signer::Signer};

type Factor = u128;
type Amount = u64;

/// Operations for global configurations.
pub trait ConfigOps<C> {
    /// Toggle feature.
    fn toggle_feature(
        &self,
        store: &Pubkey,
        domian: DomainDisabledFlag,
        action: ActionDisabledFlag,
        enable: bool,
    ) -> TransactionBuilder<C>;

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

impl<C: Deref<Target = impl Signer> + Clone> ConfigOps<C> for crate::Client<C> {
    fn toggle_feature(
        &self,
        store: &Pubkey,
        domian: DomainDisabledFlag,
        action: ActionDisabledFlag,
        enable: bool,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::ToggleFeature {
                domain: domian.to_string(),
                action: action.to_string(),
                enable,
            })
            .anchor_accounts(accounts::ToggleFeature {
                authority: self.payer(),
                store: *store,
            })
    }

    fn insert_global_amount(
        &self,
        store: &Pubkey,
        key: &str,
        amount: &Amount,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(args::InsertAmount {
                key: key.to_string(),
                amount: *amount,
            })
            .anchor_accounts(accounts::InsertAmount {
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
            .anchor_args(args::InsertFactor {
                key: key.to_string(),
                factor: *factor,
            })
            .anchor_accounts(accounts::InsertFactor {
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
            .anchor_args(args::InsertAddress {
                key: key.to_string(),
                address: *address,
            })
            .anchor_accounts(accounts::InsertAddress {
                authority,
                store: *store,
            })
    }
}
