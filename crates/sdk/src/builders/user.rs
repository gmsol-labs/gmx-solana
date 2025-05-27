use gmsol_programs::{
    anchor_lang::{InstructionData, ToAccountMetas},
    gmsol_store::client::{accounts, args},
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup};
use solana_sdk::{instruction::Instruction, system_program};
use typed_builder::TypedBuilder;

use crate::serde::StringPubkey;

use super::StoreProgram;

/// Prepare user account.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct PrepareUser {
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
}

impl IntoAtomicGroup for PrepareUser {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let user = self.program.find_user_address(&owner);
        Ok(AtomicGroup::with_instructions(
            &owner,
            Some(Instruction {
                program_id: self.program.id.0,
                accounts: accounts::PrepareUser {
                    owner,
                    store: self.program.store.0,
                    user,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                data: args::PrepareUser {}.data(),
            }),
        ))
    }
}
