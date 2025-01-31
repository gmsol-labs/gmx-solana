use std::ops::Deref;

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};

use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{accounts, instruction};

/// Roles management for GMSOL.
pub trait RolesOps<C> {
    /// Enable a role.
    fn enable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Disable a role.
    fn disable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Grant a role to user.
    fn grant_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Revoke a role from the user.
    fn revoke_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C>;
}

impl<C, S> RolesOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn enable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::EnableRole {
                authority,
                store: *store,
            })
            .anchor_args(instruction::EnableRole {
                role: role.to_string(),
            })
    }

    fn disable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::DisableRole {
                authority: self.payer(),
                store: *store,
            })
            .anchor_args(instruction::DisableRole {
                role: role.to_string(),
            })
    }

    fn grant_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::GrantRole {
                authority,
                store: *store,
            })
            .anchor_args(instruction::GrantRole {
                user: *user,
                role: role.to_string(),
            })
    }

    fn revoke_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(instruction::RevokeRole {
                user: *user,
                role: role.to_string(),
            })
            .anchor_accounts(accounts::RevokeRole {
                authority: self.payer(),
                store: *store,
            })
    }
}
