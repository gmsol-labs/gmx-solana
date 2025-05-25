use std::ops::Deref;

use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

/// System Program Ops.
pub trait SystemProgramOps<C> {
    /// Transfer to.
    fn transfer(&self, to: &Pubkey, lamports: u64) -> crate::Result<TransactionBuilder<C>>;
}

impl<C: Clone + Deref<Target = impl Signer>> SystemProgramOps<C> for crate::Client<C> {
    fn transfer(&self, to: &Pubkey, lamports: u64) -> crate::Result<TransactionBuilder<C>> {
        use solana_sdk::system_instruction::transfer;

        if lamports == 0 {
            return Err(crate::Error::custom("transferring amount is zero"));
        }
        Ok(self
            .store_transaction()
            .pre_instruction(transfer(&self.payer(), to, lamports), false))
    }
}
