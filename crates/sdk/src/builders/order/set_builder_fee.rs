use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

/// Builder for the `set_builder_fee` instruction.
///
/// Checkpoints a builder and its fee factor onto a pending order. The order's
/// owner must sign, and the factor must match what the builder currently
/// advertises, so the caller has to read that factor before building this
/// instruction rather than letting the program pick it up implicitly.
///
/// To cancel a builder fee, checkpoint a User Account advertising `0`. The
/// owner's own User Account does so until its owner sets a factor, which makes
/// it the natural choice.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetBuilderFee {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. the order's owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Order to checkpoint the builder fee onto.
    #[builder(setter(into))]
    pub order: StringPubkey,
    /// The builder's User Account.
    #[builder(setter(into))]
    pub builder: StringPubkey,
    /// The factor the builder is expected to be advertising.
    ///
    /// The instruction fails unless it matches exactly, which is what prevents
    /// the builder from raising its rate after the owner has signed.
    pub expected_factor: u128,
}

/// Hint for [`SetBuilderFee`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetBuilderFeeHint {
    /// The order's final output token.
    ///
    /// Read from the order account. An order whose final output token is
    /// uninitialized cannot carry a builder fee, so there is no hint to build
    /// here and the instruction would be rejected anyway.
    #[builder(setter(into))]
    pub final_output_token: StringPubkey,
}

impl IntoAtomicGroup for SetBuilderFee {
    type Hint = SetBuilderFeeHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;
        let builder = self.builder.0;
        let final_output_token = hint.final_output_token.0;
        // The order path is legacy-SPL throughout: order escrows and the claim
        // vault are `Account<TokenAccount>`, not the token-interface flavour.
        let token_program_id = anchor_spl::token::ID;

        // The claim vault is the builder's ATA for the fee token. It has to
        // exist already: this instruction only checks for it, and creating it
        // here would charge the order's owner rent for the builder's account.
        let claim_vault: Pubkey = get_associated_token_address_with_program_id(
            &builder,
            &final_output_token,
            &token_program_id,
        );

        let set = self
            .program
            .anchor_instruction(args::SetBuilderFee {
                expected_factor: self.expected_factor,
            })
            .anchor_accounts(
                accounts::SetBuilderFee {
                    owner: payer,
                    store: self.program.store.0,
                    order: self.order.0,
                    builder,
                    final_output_token,
                    claim_vault,
                    user_token_controller: self
                        .program
                        .find_user_token_controller_address(&builder, &final_output_token),
                    token_program: token_program_id,
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                },
                true,
            )
            .build();

        Ok(AtomicGroup::with_instructions(&payer, Some(set)))
    }
}
