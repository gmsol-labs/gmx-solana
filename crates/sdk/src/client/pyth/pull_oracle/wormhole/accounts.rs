use gmsol_programs::anchor_lang::ToAccountMetas;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

pub(super) struct InitEncodedVaa {
    pub(super) write_authority: Pubkey,
    pub(super) encoded_vaa: Pubkey,
}

impl ToAccountMetas for InitEncodedVaa {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.write_authority, true),
            AccountMeta::new(self.encoded_vaa, false),
        ]
    }
}

pub(super) struct WriteEncodedVaa {
    pub(super) write_authority: Pubkey,
    pub(super) draft_vaa: Pubkey,
}

impl ToAccountMetas for WriteEncodedVaa {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.write_authority, true),
            AccountMeta::new(self.draft_vaa, false),
        ]
    }
}

pub(super) struct VerifyEncodedVaaV1 {
    pub(super) write_authority: Pubkey,
    pub(super) draft_vaa: Pubkey,
    pub(super) guardian_set: Pubkey,
}

impl ToAccountMetas for VerifyEncodedVaaV1 {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.write_authority, true),
            AccountMeta::new(self.draft_vaa, false),
            AccountMeta::new_readonly(self.guardian_set, false),
        ]
    }
}

pub(super) struct CloseEncodedVaa {
    pub(super) write_authority: Pubkey,
    pub(super) encoded_vaa: Pubkey,
}

impl ToAccountMetas for CloseEncodedVaa {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.write_authority, true),
            AccountMeta::new(self.encoded_vaa, false),
        ]
    }
}
