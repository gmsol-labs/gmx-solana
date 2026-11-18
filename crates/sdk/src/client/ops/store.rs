use std::{ops::Deref, sync::Arc};

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

/// Operations for store account.
pub trait StoreOps<C> {
    /// Initialize [`Store`](gmsol_programs::gmsol_store::accounts::Store) account.
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

    /// Initialize callback authority.
    fn initialize_callback_authority(&self) -> TransactionBuilder<C>;

    /// Claim the receiver's whole rent collector balance to a chosen destination.
    ///
    /// The receiver must sign; attach its signer if it differs from this client's
    /// payer. No balance read, Store account or initialization is needed. Empty
    /// claims are no-ops; nonempty claims reject the collector as destination.
    fn claim_rent(&self, rent_receiver: &Pubkey, destination: &Pubkey) -> TransactionBuilder<C>;

    /// Update last restarted slot.
    fn update_last_restarted_slot(&self, store: &Pubkey) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> StoreOps<C> for crate::Client<C> {
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
            .anchor_args(args::Initialize {
                key: key.to_string(),
            });

        for signer in authority.into_iter().chain(receiver).chain(holding) {
            rpc = rpc.owned_signer(Arc::new(signer));
        }

        rpc
    }

    fn transfer_store_authority(
        &self,
        store: &Pubkey,
        next_authority: &Pubkey,
    ) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::TransferStoreAuthority {})
            .anchor_accounts(accounts::TransferStoreAuthority {
                authority: self.payer(),
                store: *store,
                next_authority: *next_authority,
            })
    }

    fn accept_store_authority(&self, store: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::AcceptStoreAuthority {})
            .anchor_accounts(accounts::AcceptStoreAuthority {
                next_authority: self.payer(),
                store: *store,
            })
    }

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::TransferReceiver {})
            .anchor_accounts(accounts::TransferReceiver {
                authority: self.payer(),
                store: *store,
                next_receiver: *new_receiver,
            })
    }

    fn set_token_map(&self, store: &Pubkey, token_map: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::SetTokenMap {})
            .anchor_accounts(accounts::SetTokenMap {
                authority: self.payer(),
                store: *store,
                token_map: *token_map,
            })
    }

    fn initialize_callback_authority(&self) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::InitializeCallbackAuthority {})
            .anchor_accounts(accounts::InitializeCallbackAuthority {
                payer: self.payer(),
                callback_authority: self.find_callback_authority_address(),
                system_program: system_program::ID,
            })
    }

    fn claim_rent(&self, rent_receiver: &Pubkey, destination: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::ClaimRent {})
            .anchor_accounts(accounts::ClaimRent {
                rent_receiver: *rent_receiver,
                rent_collector: self.find_rent_collector_address(rent_receiver),
                destination: *destination,
                system_program: system_program::ID,
            })
    }

    fn update_last_restarted_slot(&self, store: &Pubkey) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(args::UpdateLastRestartedSlot {})
            .anchor_accounts(accounts::UpdateLastRestartedSlot {
                authority,
                store: *store,
            })
    }
}

#[cfg(test)]
mod rent_tests {
    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::{instruction::AccountMeta, signature::Keypair};

    use super::*;

    #[test]
    fn rent_claim_uses_exact_accounts_and_configured_program() -> crate::Result<()> {
        let receiver = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        for program in [gmsol_programs::gmsol_store::ID, Pubkey::new_unique()] {
            let client = crate::Client::new_with_options(
                Cluster::Localnet,
                Arc::new(Keypair::new()),
                crate::client::ClientOptions::builder()
                    .store_program_id(Some(program))
                    .build(),
            )?;
            let collector =
                Pubkey::find_program_address(&[b"rent_collector", receiver.as_ref()], &program).0;
            assert_eq!(client.find_rent_collector_address(&receiver), collector);
            let ix = client
                .claim_rent(&receiver, &destination)
                .take_instruction()
                .expect("claim instruction missing");
            assert_eq!(ix.program_id, program);
            assert_eq!(
                ix.accounts,
                vec![
                    AccountMeta::new_readonly(receiver, true),
                    AccountMeta::new(collector, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new_readonly(system_program::ID, false),
                ]
            );
            assert_eq!(
                ix.data,
                solana_sdk::hash::hash(b"global:claim_rent").to_bytes()[..8]
            );
        }
        Ok(())
    }
}
