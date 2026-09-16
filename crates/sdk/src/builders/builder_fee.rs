use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

/// Builder for the `claim_builder_fees` instruction.
///
/// Sweeps the whole balance of the payer's own claim vault for one token mint
/// into a destination token account. The program restricts the call to the
/// payer's own User Account, so there is nothing to authorize beyond the
/// signature and no hint to fetch: every account is a pure function of the
/// store, the mint, the destination and the payer's key.
///
/// The claim vault has to exist. A builder that has never been settled for
/// this mint therefore fails here rather than succeeding with nothing moved,
/// which is the more useful answer for a caller who asked to be paid.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct ClaimBuilderFees {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer, i.e. the builder claiming its own fees.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// The mint the fees are denominated in.
    ///
    /// One claim vault exists per `(User Account, mint)`, so a builder owed
    /// fees in several mints claims each of them separately.
    #[builder(setter(into))]
    pub token_mint: StringPubkey,
    /// Token account to receive the claimed balance.
    ///
    /// Chosen freely by the caller and not derived, so it must already exist
    /// and match `token_mint`.
    #[builder(setter(into))]
    pub destination: StringPubkey,
}

impl IntoAtomicGroup for ClaimBuilderFees {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = self.payer.0;
        let token_mint = self.token_mint.0;
        let user_account = self.program.find_user_address(&owner);
        // The builder fee path is legacy-SPL throughout: the claim vault is an
        // `Account<TokenAccount>`, not the token-interface flavour.
        let token_program_id = anchor_spl::token::ID;

        // The claim vault is the User Account's ATA for the mint, which is what
        // makes the radius of a frozen vault per `(builder_user, mint)` rather
        // than per builder.
        let claim_vault = get_associated_token_address_with_program_id(
            &user_account,
            &token_mint,
            &token_program_id,
        );

        let claim = self
            .program
            .anchor_instruction(args::ClaimBuilderFees {})
            .anchor_accounts(
                accounts::ClaimBuilderFees {
                    owner,
                    store: self.program.store.0,
                    user_account,
                    token_mint,
                    claim_vault,
                    destination: self.destination.0,
                    user_token_controller: self
                        .program
                        .find_user_token_controller_address(&user_account, &token_mint),
                    token_program: token_program_id,
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                },
                false,
            )
            .build();

        Ok(AtomicGroup::with_instructions(&owner, Some(claim)))
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    /// The claim vault belongs to the **User Account**, not to the owner's own
    /// key. Deriving it from the owner compiles, produces a valid-looking ATA
    /// and is wrong, so pin it.
    #[test]
    fn claim_vault_and_controller_derive_from_the_user_account() -> crate::Result<()> {
        let payer = Pubkey::new_unique();
        let token_mint = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let claim = ClaimBuilderFees::builder()
            .payer(payer)
            .token_mint(token_mint)
            .destination(destination)
            .build();
        let program = claim.program.clone();

        let user_account = program.find_user_address(&payer);
        let expected_vault = get_associated_token_address_with_program_id(
            &user_account,
            &token_mint,
            &anchor_spl::token::ID,
        );
        let owner_ata = get_associated_token_address_with_program_id(
            &payer,
            &token_mint,
            &anchor_spl::token::ID,
        );
        assert_ne!(
            expected_vault, owner_ata,
            "the two derivations must differ, otherwise this test proves nothing"
        );

        let group = claim.into_atomic_group(&())?;
        let keys: Vec<Pubkey> = group
            .instructions_with_options(Default::default())
            .filter(|ix| ix.program_id == program.id.0)
            .flat_map(|ix| ix.accounts.iter().map(|m| m.pubkey).collect::<Vec<_>>())
            .collect();

        assert!(keys.contains(&user_account), "user account missing");
        assert!(
            keys.contains(&expected_vault),
            "claim vault is not the user account's ATA"
        );
        assert!(
            !keys.contains(&owner_ata),
            "claim vault was derived from the owner key"
        );
        assert!(
            keys.contains(&program.find_user_token_controller_address(&user_account, &token_mint)),
            "token controller is not derived from the user account"
        );
        assert!(keys.contains(&destination), "destination missing");
        Ok(())
    }
}
