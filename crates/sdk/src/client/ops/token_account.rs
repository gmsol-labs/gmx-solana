use std::ops::Deref;

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

    /// Close a claimable account if it is empty.
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

    /// Create token metadata for a token whose mint authority is `store`.
    fn create_token_metadata(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        name: String,
        symbol: String,
        uri: String,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Update a token metadata whose update authority is `store`.
    fn update_token_metadata(
        &self,
        store: &Pubkey,
        metadata: &Pubkey,
        name: String,
        symbol: String,
        uri: String,
    ) -> TransactionBuilder<C>;

    /// Update a token metadata whose update authority is `store` for the give mint.
    fn update_token_metadata_by_mint(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        name: String,
        symbol: String,
        uri: String,
    ) -> TransactionBuilder<C> {
        let metadata = find_token_metadata_address(mint);
        self.update_token_metadata(store, &metadata, name, symbol, uri)
    }
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
        use anchor_spl::associated_token::spl_associated_token_account;

        let payer = self.payer();
        let owner = owner.copied().unwrap_or(payer);
        let ix =
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &payer,
                &owner,
                mint,
                token_program_id,
            );
        self.store_transaction()
            .program(spl_associated_token_account::ID)
            .pre_instruction(ix, true)
    }

    fn create_token_metadata(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        name: String,
        symbol: String,
        uri: String,
    ) -> TransactionBuilder<C, Pubkey> {
        let authority = self.payer();
        let metadata = find_token_metadata_address(mint);
        self.store_transaction()
            .anchor_accounts(accounts::CreateTokenMetadata {
                authority,
                store: *store,
                mint: *mint,
                metadata,
                system_program: system_program::ID,
                sysvar_instructions: solana_sdk::sysvar::instructions::ID,
                metadata_program: anchor_spl::metadata::ID,
            })
            .anchor_args(args::CreateTokenMetadata { name, symbol, uri })
            .output(metadata)
    }

    fn update_token_metadata(
        &self,
        store: &Pubkey,
        metadata: &Pubkey,
        name: String,
        symbol: String,
        uri: String,
    ) -> TransactionBuilder<C> {
        let authority = self.payer();
        self.store_transaction()
            .anchor_accounts(accounts::UpdateTokenMetadata {
                authority,
                store: *store,
                metadata: *metadata,
                metadata_program: anchor_spl::metadata::ID,
            })
            .anchor_args(args::UpdateTokenMetadata { name, symbol, uri })
    }
}

const TOKEN_METADATA_SEED: &[u8] = b"metadata";

fn find_token_metadata_address(mint: &Pubkey) -> Pubkey {
    let program_id = &anchor_spl::metadata::ID;
    Pubkey::find_program_address(
        &[TOKEN_METADATA_SEED, program_id.as_ref(), mint.as_ref()],
        program_id,
    )
    .0
}
