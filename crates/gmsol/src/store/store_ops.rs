use std::ops::Deref;

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_store::{
    accounts, instruction,
    states::{Factor, FactorKey},
};

use crate::utils::RpcBuilder;

/// Data Store management for GMSOL.
pub trait StoreOps<C> {
    /// Initialize [`Store`](gmsol_store::states::Store) account.
    fn initialize_store(&self, key: &str, authority: Option<&Pubkey>) -> RpcBuilder<C>;

    /// Transfer Store authority.
    fn transfer_store_authority(&self, store: &Pubkey, new_authority: &Pubkey) -> RpcBuilder<C>;

    /// Transfer receiver.
    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C>;

    /// Set new token map.
    fn set_token_map(&self, store: &Pubkey, token_map: &Pubkey) -> RpcBuilder<C>;

    /// Insert factor.
    fn insert_factor(&self, store: &Pubkey, key: FactorKey, factor: Factor) -> RpcBuilder<C>;
}

impl<C, S> StoreOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn initialize_store(&self, key: &str, authority: Option<&Pubkey>) -> RpcBuilder<C> {
        let store = self.find_store_address(key);
        self.store_rpc()
            .accounts(accounts::Initialize {
                payer: self.payer(),
                store,
                system_program: system_program::ID,
            })
            .args(instruction::Initialize {
                key: key.to_string(),
                authority: authority.copied(),
            })
    }

    fn transfer_store_authority(&self, store: &Pubkey, new_authority: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::TransferStoreAuthority {
                new_authority: *new_authority,
            })
            .accounts(accounts::TransferStoreAuthority {
                authority: self.payer(),
                store: *store,
            })
    }

    fn transfer_receiver(&self, store: &Pubkey, new_receiver: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::SetReceiver {})
            .accounts(accounts::SetReceiver {
                authority: self.payer(),
                store: *store,
                receiver: *new_receiver,
            })
    }

    fn set_token_map(&self, store: &Pubkey, token_map: &Pubkey) -> RpcBuilder<C> {
        self.store_rpc()
            .args(instruction::SetTokenMap {})
            .accounts(accounts::SetTokenMap {
                authority: self.payer(),
                store: *store,
                token_map: *token_map,
            })
    }

    fn insert_factor(&self, store: &Pubkey, key: FactorKey, factor: Factor) -> RpcBuilder<C> {
        let rpc = self.store_rpc().accounts(accounts::InsertConfig {
            authority: self.payer(),
            store: *store,
        });
        match key {
            FactorKey::OrderFeeDiscountForReferredUser => {
                rpc.args(instruction::InsertOrderFeeDiscountForReferredUser { factor })
            }
            _ => rpc.args(instruction::InsertFactor {
                key: key.to_string(),
                factor,
            }),
        }
    }
}
