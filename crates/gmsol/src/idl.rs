use std::ops::Deref;

use anchor_client::anchor_lang::{
    idl::{IdlAccount, IdlInstruction, IDL_IX_TAG},
    AnchorSerialize,
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey, signer::Signer};

/// IDL operations.
pub trait IdlOps<C> {
    /// Create IDL account.
    fn create_idl_account(
        &self,
        program_id: &Pubkey,
        data_len: u64,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Resize buffer/account.
    fn resize_idl_account(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        data_len: u64,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Set IDL buffer.
    fn set_idl_buffer(
        &self,
        program_id: &Pubkey,
        buffer: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Set IDL authority.
    fn set_idl_authority(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        new_authority: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Close IDL buffer/account.
    fn close_idl_account(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        sol_destination: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>>;
}

fn serialize_idl_ix(ix: IdlInstruction) -> crate::Result<Vec<u8>> {
    let mut data = IDL_IX_TAG.to_le_bytes().to_vec();
    data.append(&mut ix.try_to_vec()?);
    Ok(data)
}

impl<C: Deref<Target = impl Signer> + Clone> IdlOps<C> for crate::Client<C> {
    fn create_idl_account(
        &self,
        program_id: &Pubkey,
        data_len: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        let idl_address = IdlAccount::address(program_id);
        let program_signer = Pubkey::find_program_address(&[], program_id).0;

        let tx = self
            .store_transaction()
            .program(*program_id)
            .accounts(vec![
                AccountMeta::new_readonly(self.payer(), true),
                AccountMeta::new(idl_address, false),
                AccountMeta::new_readonly(program_signer, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                AccountMeta::new_readonly(*program_id, false),
            ])
            .args(serialize_idl_ix(IdlInstruction::Create { data_len })?);

        Ok(tx)
    }

    fn resize_idl_account(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        data_len: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        let account = account
            .copied()
            .unwrap_or_else(|| IdlAccount::address(program_id));

        let tx = self
            .store_transaction()
            .program(*program_id)
            .accounts(vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(self.payer(), true),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ])
            .args(serialize_idl_ix(IdlInstruction::Resize { data_len })?);

        Ok(tx)
    }

    fn set_idl_buffer(
        &self,
        program_id: &Pubkey,
        buffer: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let idl_address = IdlAccount::address(program_id);
        let tx = self
            .store_transaction()
            .program(*program_id)
            .accounts(vec![
                AccountMeta::new(*buffer, false),
                AccountMeta::new(idl_address, false),
                AccountMeta::new(self.payer(), true),
            ])
            .args(serialize_idl_ix(IdlInstruction::SetBuffer)?);

        Ok(tx)
    }

    fn set_idl_authority(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        new_authority: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let idl_address = account
            .copied()
            .unwrap_or_else(|| IdlAccount::address(program_id));

        let tx = self
            .store_transaction()
            .program(*program_id)
            .accounts(vec![
                AccountMeta::new(idl_address, false),
                AccountMeta::new_readonly(self.payer(), true),
            ])
            .args(serialize_idl_ix(IdlInstruction::SetAuthority {
                new_authority: *new_authority,
            })?);

        Ok(tx)
    }

    fn close_idl_account(
        &self,
        program_id: &Pubkey,
        account: Option<&Pubkey>,
        sol_destination: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let idl_address = account
            .copied()
            .unwrap_or_else(|| IdlAccount::address(program_id));

        let sol_destination = sol_destination.copied().unwrap_or_else(|| self.payer());
        let tx = self
            .store_transaction()
            .program(*program_id)
            .accounts(vec![
                AccountMeta::new(idl_address, false),
                AccountMeta::new(self.payer(), true),
                AccountMeta::new(sol_destination, false),
            ])
            .args(serialize_idl_ix(IdlInstruction::Close)?);
        Ok(tx)
    }
}
