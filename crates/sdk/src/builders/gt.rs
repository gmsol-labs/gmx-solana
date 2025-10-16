use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use typed_builder::TypedBuilder;

use crate::serde::StringPubkey;

use super::StoreProgram;

/// Builder for `mint_gt_reward` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct MintGtReward {
    /// Payer (a.k.a. authority).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// The owner for whom the GT reward will be minted.
    pub owner: StringPubkey,
    /// The amount to mint.
    pub amount: u64,
}

impl IntoAtomicGroup for MintGtReward {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let authority = self.payer.0;
        let ix = self
            .store_program
            .anchor_instruction(args::MintGtReward {
                amount: self.amount,
            })
            .anchor_accounts(
                accounts::MintGtReward {
                    authority,
                    store: self.store_program.store.0,
                    user: self.store_program.find_user_address(&self.owner),
                    event_authority: self.store_program.find_event_authority_address(),
                    program: self.store_program.id.0,
                },
                false,
            )
            .build();
        Ok(AtomicGroup::with_instructions(&authority, [ix]))
    }
}
