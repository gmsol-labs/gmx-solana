use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup};
use typed_builder::TypedBuilder;

use crate::serde::StringPubkey;

use super::StoreProgram;

/// Builder for the `close_empty_position` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CloseEmptyPosition {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Position to close.
    #[builder(setter(into))]
    pub position: StringPubkey,
}

impl IntoAtomicGroup for CloseEmptyPosition {
    type Hint = ();

    fn into_atomic_group(
        self,
        _hint: &Self::Hint,
    ) -> gmsol_solana_utils::Result<gmsol_solana_utils::AtomicGroup> {
        let owner = self.payer.0;

        let ix = self
            .program
            .instruction(args::CloseEmptyPosition {})
            .accounts(
                accounts::CloseEmptyPosition {
                    owner,
                    store: self.program.store.0,
                    position: self.position.0,
                },
                false,
            )
            .build();

        Ok(AtomicGroup::with_instructions(&owner, [ix]))
    }
}

#[cfg(test)]
mod tests {
    use gmsol_solana_utils::transaction_builder::default_before_sign;
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn close_empty_position() -> crate::Result<()> {
        CloseEmptyPosition::builder()
            .payer(Pubkey::new_unique())
            .position(Pubkey::new_unique())
            .build()
            .into_atomic_group(&())?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )?;

        Ok(())
    }
}
