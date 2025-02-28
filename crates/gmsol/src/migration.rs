use std::ops::Deref;

use anchor_client::anchor_lang::system_program;
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{accounts, instruction};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer};

/// Migration instruction.
pub trait MigrationOps<C> {
    /// Migrate referral code.
    fn migrate_referral_code(&self, store: &Pubkey, code: &Pubkey) -> TransactionBuilder<C>;
}

impl<S, C> MigrationOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn migrate_referral_code(&self, store: &Pubkey, code: &Pubkey) -> TransactionBuilder<C> {
        self.store_transaction()
            .anchor_accounts(accounts::MigrateReferralCode {
                authority: self.payer(),
                store: *store,
                system: system_program::ID,
            })
            .accounts(vec![AccountMeta::new(*code, false)])
            .anchor_args(instruction::MigrateReferralCode {})
    }
}
