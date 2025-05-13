use std::ops::Deref;

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

use crate::client::Client;

/// Token account operations.
pub trait TokenAccountOps<C> {
    /// Prepare a claimable account.
    fn use_claimable_account(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        user: &Pubkey,
        timestamp: i64,
        account: &Pubkey,
        amount: u64,
    ) -> TransactionBuilder<C>;

    /// Close a claimable account if it is emtpy.
    fn close_empty_claimable_account(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        user: &Pubkey,
        timestamp: i64,
        account: &Pubkey,
    ) -> TransactionBuilder<C>;

    /// Prepare associated token account.
    fn prepare_associated_token_account(
        &self,
        mint: &Pubkey,
        token_program_id: &Pubkey,
        owner: Option<&Pubkey>,
    ) -> TransactionBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> TokenAccountOps<C> for Client<C> {
    fn use_claimable_account(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        owner: &Pubkey,
        timestamp: i64,
        account: &Pubkey,
        amount: u64,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(args::UseClaimableAccount { timestamp, amount })
            .anchor_accounts(accounts::UseClaimableAccount {
                authority,
                store: *store,
                mint: *mint,
                owner: *owner,
                account: *account,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
            })
    }

    fn close_empty_claimable_account(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        owner: &Pubkey,
        timestamp: i64,
        account: &Pubkey,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_args(args::CloseEmptyClaimableAccount { timestamp })
            .anchor_accounts(accounts::CloseEmptyClaimableAccount {
                authority,
                store: *store,
                mint: *mint,
                owner: *owner,
                account: *account,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
            })
    }

    fn prepare_associated_token_account(
        &self,
        mint: &Pubkey,
        token_program_id: &Pubkey,
        owner: Option<&Pubkey>,
    ) -> TransactionBuilder<C> {
        let payer = self.payer();
        let owner = owner.copied().unwrap_or(payer);
        let account = get_associated_token_address_with_program_id(&owner, mint, token_program_id);
        self.store_transaction()
            .anchor_accounts(accounts::PrepareAssociatedTokenAccount {
                payer,
                owner,
                mint: *mint,
                account,
                system_program: system_program::ID,
                token_program: *token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(args::PrepareAssociatedTokenAccount {})
    }
}
