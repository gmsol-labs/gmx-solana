use std::ops::Deref;

use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

/// Operations for role management.
pub trait RoleOps<C> {
    /// Enable a role.
    fn enable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Disable a role.
    fn disable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Grant a role to user.
    fn grant_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C>;

    /// Revoke a role from the user.
    fn revoke_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> RoleOps<C> for crate::Client<C> {
    fn enable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::EnableRole {
                authority,
                store: *store,
            })
            .anchor_args(args::EnableRole {
                role: role.to_string(),
            })
    }

    fn disable_role(&self, store: &Pubkey, role: &str) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::DisableRole {
                authority: self.payer(),
                store: *store,
            })
            .anchor_args(args::DisableRole {
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
            .anchor_args(args::GrantRole {
                user: *user,
                role: role.to_string(),
            })
    }

    fn revoke_role(&self, store: &Pubkey, user: &Pubkey, role: &str) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_args(args::RevokeRole {
                user: *user,
                role: role.to_string(),
            })
            .anchor_accounts(accounts::RevokeRole {
                authority: self.payer(),
                store: *store,
            })
    }
}
