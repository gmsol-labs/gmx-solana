use gmsol_programs::{
    anchor_lang::{InstructionData, ToAccountMetas},
    gmsol_store::client::{accounts, args},
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
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

/// Builder for the `set_builder_fee_factor` instruction.
///
/// Sets the fee factor the payer's User Account advertises to order owners.
/// The factor is a ceiling enforced by the store's `MaxBuilderFeeFactor`,
/// which reads `0` until a config keeper raises it, so a builder cannot
/// advertise anything until that happens.
///
/// Advertising `0` opts out. It is also what an order owner checkpoints to
/// cancel a builder fee, since every User Account advertises `0` until its
/// owner sets a factor.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetBuilderFeeFactor {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer, i.e. the builder whose own User Account advertises the factor.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// The factor to advertise.
    ///
    /// Rejected if it exceeds the store's `MaxBuilderFeeFactor`.
    pub factor: u128,
}

impl IntoAtomicGroup for SetBuilderFeeFactor {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;

        let set = self
            .program
            .anchor_instruction(args::SetBuilderFeeFactor {
                factor: self.factor,
            })
            .anchor_accounts(
                accounts::SetBuilderFeeFactor {
                    owner,
                    store: self.program.store.0,
                    user: self.program.find_user_address(&owner),
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                },
                false,
            )
            .build();

        Ok(AtomicGroup::with_instructions(&owner, Some(set)))
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    /// The factor is advertised by the owner's **User Account**, a PDA, not by
    /// the owner's own key. Passing the owner where the PDA belongs is the
    /// plausible slip here, so pin the derivation.
    #[test]
    fn factor_is_written_to_the_owners_user_account() -> crate::Result<()> {
        let payer = Pubkey::new_unique();

        let set = SetBuilderFeeFactor::builder()
            .payer(payer)
            .factor(42)
            .build();
        let program = set.program.clone();
        let user = program.find_user_address(&payer);
        assert_ne!(
            user, payer,
            "the PDA must differ from the owner, otherwise this test proves nothing"
        );

        let group = set.into_atomic_group(&())?;
        let keys: Vec<Pubkey> = group
            .instructions_with_options(Default::default())
            .filter(|ix| ix.program_id == program.id.0)
            .flat_map(|ix| ix.accounts.iter().map(|m| m.pubkey).collect::<Vec<_>>())
            .collect();

        assert!(keys.contains(&user), "the user account PDA is missing");
        assert!(keys.contains(&payer), "the owner must still sign");
        Ok(())
    }
}
