use gmsol_programs::anchor_lang::ToAccountMetas;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

pub(super) struct PostUpdate {
    pub(super) payer: Pubkey,
    pub(super) encoded_vaa: Pubkey,
    pub(super) config: Pubkey,
    pub(super) treasury: Pubkey,
    pub(super) price_update_account: Pubkey,
    pub(super) system_program: Pubkey,
    pub(super) write_authority: Pubkey,
}

impl ToAccountMetas for PostUpdate {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.payer, true),
            AccountMeta::new_readonly(self.encoded_vaa, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.treasury, false),
            AccountMeta::new(self.price_update_account, true),
            AccountMeta::new_readonly(self.system_program, false),
            AccountMeta::new_readonly(self.write_authority, true),
        ]
    }
}

pub(super) struct ReclaimRent {
    pub(super) payer: Pubkey,
    pub(super) price_update_account: Pubkey,
}

impl ToAccountMetas for ReclaimRent {
    fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.payer, true),
            AccountMeta::new(self.price_update_account, false),
        ]
    }
}
