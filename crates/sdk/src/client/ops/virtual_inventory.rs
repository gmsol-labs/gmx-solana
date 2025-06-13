use std::ops::Deref;

use anchor_lang::system_program;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

/// Operations for virtual inventory accounts.
pub trait VirtualInventoryOps<C> {
    /// Close a virtual inventory account.
    fn close_virtual_inventory_account(
        &self,
        store: &Pubkey,
        virtual_inventory: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Create a virtual inventory for swaps account.
    fn create_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        index: u32,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>>;

    /// Join a virtual inventory for swaps.
    fn join_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        market: &Pubkey,
        virtual_inventory_for_swaps: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Leave a virtual inventory for swaps.
    fn leave_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        market: &Pubkey,
        virtual_inventory_for_swaps: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;
}

impl<C: Deref<Target = impl Signer> + Clone> VirtualInventoryOps<C> for crate::Client<C> {
    fn close_virtual_inventory_account(
        &self,
        store: &Pubkey,
        virtual_inventory: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let txn = self
            .store_transaction()
            .anchor_accounts(accounts::CloseVirtualInventory {
                authority: self.payer(),
                store: *store,
                store_wallet: self.find_store_wallet_address(store),
                virtual_inventory: *virtual_inventory,
            })
            .anchor_args(args::CloseVirtualInventory {});

        Ok(txn)
    }

    fn create_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        index: u32,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let virtual_inventory = self.find_virtual_inventory_for_swaps_address(store, index);
        let txn = self
            .store_transaction()
            .anchor_accounts(accounts::CreateVirtualInventoryForSwaps {
                authority: self.payer(),
                store: *store,
                virtual_inventory,
                system_program: system_program::ID,
            })
            .anchor_args(args::CreateVirtualInventoryForSwaps { index })
            .output(virtual_inventory);
        Ok(txn)
    }

    fn join_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        market: &Pubkey,
        virtual_inventory_for_swaps: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let txn = self
            .store_transaction()
            .anchor_accounts(accounts::JoinVirtualInventoryForSwaps {
                authority: self.payer(),
                store: *store,
                virtual_inventory: *virtual_inventory_for_swaps,
                market: *market,
            })
            .anchor_args(args::JoinVirtualInventoryForSwaps {});

        Ok(txn)
    }

    fn leave_virtual_inventory_for_swaps(
        &self,
        store: &Pubkey,
        market: &Pubkey,
        virtual_inventory_for_swaps: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let txn = self
            .store_transaction()
            .anchor_accounts(accounts::LeaveVirtualInventoryForSwaps {
                authority: self.payer(),
                store: *store,
                virtual_inventory: *virtual_inventory_for_swaps,
                market: *market,
            })
            .anchor_args(args::LeaveVirtualInventoryForSwaps {});

        Ok(txn)
    }
}
