use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

const ERR_NO_MINT: &str = "order has a non-zero builder fee amount but no final output token mint";
const ERR_NO_ESCROW: &str =
    "order has a non-zero builder fee amount but no final output token escrow";
const ERR_NO_BUILDER: &str = "order has a non-zero builder fee amount but no builder recorded";

/// Builder for the `settle_builder_fee` instruction.
///
/// Moves an order's recorded builder fee out of the final output token escrow
/// and into the builder's claim vault. Permissionless and idempotent: safe to
/// call on any order, in any state, whether or not it carries a non-zero
/// recorded amount.
///
/// The payer signs only to pay the transaction. The instruction itself takes no
/// authority account, which is what makes a keeper able to settle on anyone's
/// behalf.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SettleBuilderFee {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer, who signs for the transaction fee only.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Order whose builder fee is being settled.
    #[builder(setter(into))]
    pub order: StringPubkey,
}

/// Hint for [`SettleBuilderFee`].
///
/// Every field is read off the order account. A zero `builder_fee_amount`
/// makes the other three irrelevant, because settlement is then a no-op that
/// performs no CPI and touches none of them.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SettleBuilderFeeHint {
    /// The order's recorded builder fee amount.
    pub builder_fee_amount: u64,
    /// The builder's User Account, if the order has one attached.
    #[builder(setter(into))]
    pub builder: Option<StringPubkey>,
    /// The order's final output token mint, i.e. the token the fee is
    /// denominated in.
    ///
    /// `None` for an order that never had one, which is a supported state.
    #[builder(setter(into))]
    pub final_output_token: Option<StringPubkey>,
    /// The order's escrow account for the final output token.
    ///
    /// `None` when it was never initialized. Also a supported state rather
    /// than an error.
    #[builder(setter(into))]
    pub escrow: Option<StringPubkey>,
}

impl IntoAtomicGroup for SettleBuilderFee {
    type Hint = SettleBuilderFeeHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;

        // All four accounts are required only for a genuine settlement. On the
        // no-op path they need not even exist on-chain, so a zero amount must
        // not be turned into an error: refusing to build the instruction is
        // what would make the no-op unreachable for exactly those orders.
        //
        // Each missing one names itself. A hint derived from the order cannot
        // miss the mint without the escrow, since `TokenAndAccount` records
        // both or neither, but a caller-supplied hint can.
        let (final_output_token, escrow, builder_user, claim_vault) =
            if hint.builder_fee_amount == 0 {
                (None, None, None, None)
            } else {
                let final_output_token = hint
                    .final_output_token
                    .as_ref()
                    .ok_or_else(|| gmsol_solana_utils::Error::custom(ERR_NO_MINT))?;
                let escrow = hint
                    .escrow
                    .as_ref()
                    .ok_or_else(|| gmsol_solana_utils::Error::custom(ERR_NO_ESCROW))?;
                let builder = hint
                    .builder
                    .as_ref()
                    .ok_or_else(|| gmsol_solana_utils::Error::custom(ERR_NO_BUILDER))?;
                // The claim vault is the builder User Account's ATA for the fee
                // token, legacy-SPL like the rest of the order path.
                let claim_vault = get_associated_token_address_with_program_id(
                    &builder.0,
                    &final_output_token.0,
                    &anchor_spl::token::ID,
                );
                (
                    Some(final_output_token.0),
                    Some(escrow.0),
                    Some(builder.0),
                    Some(claim_vault),
                )
            };

        let settle = self
            .program
            .anchor_instruction(args::SettleBuilderFee {})
            .anchor_accounts(
                accounts::SettleBuilderFee {
                    store: self.program.store.0,
                    order: self.order.0,
                    final_output_token,
                    escrow,
                    builder_user,
                    claim_vault,
                    token_program: anchor_spl::token::ID,
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                },
                true,
            )
            .build();

        Ok(AtomicGroup::with_instructions(&payer, Some(settle)))
    }
}

impl FromRpcClientWith<SettleBuilderFee> for SettleBuilderFeeHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a SettleBuilderFee,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        use crate::{programs::gmsol_store::accounts::Order, utils::zero_copy::ZeroCopy};
        use gmsol_solana_utils::client_traits::RpcClientExt;
        use gmsol_utils::pubkey::optional_address;

        let order = client
            .get_anchor_account::<ZeroCopy<Order>>(&builder.order.0, Default::default())
            .await?
            .0;

        Ok(Self {
            builder_fee_amount: order.builder_fee_amount,
            builder: optional_address(&order.builder).copied().map(Into::into),
            final_output_token: order.tokens.final_output_token.token().map(Into::into),
            escrow: order.tokens.final_output_token.account().map(Into::into),
        })
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    fn hint(amount: u64) -> SettleBuilderFeeHintBuilder<((u64,), (), (), ())> {
        SettleBuilderFeeHint::builder().builder_fee_amount(amount)
    }

    fn keys_of(group: &AtomicGroup, program_id: &Pubkey) -> Vec<Pubkey> {
        group
            .instructions_with_options(Default::default())
            .filter(|ix| ix.program_id == *program_id)
            .flat_map(|ix| ix.accounts.iter().map(|m| m.pubkey).collect::<Vec<_>>())
            .collect()
    }

    /// A zero recorded amount is a supported state, not an error, and the
    /// no-op performs no CPI. The four settlement accounts must therefore stay
    /// out of the instruction even when the caller hands over a hint that
    /// carries them.
    #[test]
    fn a_zero_amount_leaves_the_settlement_accounts_out() -> crate::Result<()> {
        let payer = Pubkey::new_unique();
        let order = Pubkey::new_unique();
        let builder_user = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        let settle = SettleBuilderFee::builder()
            .payer(payer)
            .order(order)
            .build();
        let program = settle.program.clone();

        let group = settle.into_atomic_group(
            &hint(0)
                .builder(Some(builder_user.into()))
                .final_output_token(Some(mint.into()))
                .escrow(Some(escrow.into()))
                .build(),
        )?;

        let keys = keys_of(&group, &program.id.0);
        assert!(keys.contains(&order), "the order account is always present");
        for (name, key) in [
            ("builder", builder_user),
            ("mint", mint),
            ("escrow", escrow),
        ] {
            assert!(
                !keys.contains(&key),
                "{name} must not be attached on the no-op path"
            );
        }
        Ok(())
    }

    /// The three missing-field errors used to share one message, which made a
    /// caller-supplied hint impossible to debug. Each has to name itself.
    #[test]
    fn each_missing_hint_field_names_itself() {
        let payer = Pubkey::new_unique();
        let order = Pubkey::new_unique();
        let some = || Some(StringPubkey::from(Pubkey::new_unique()));

        let cases = [
            (
                "no final output token mint",
                hint(1)
                    .builder(some())
                    .final_output_token(None)
                    .escrow(some())
                    .build(),
            ),
            (
                "no final output token escrow",
                hint(1)
                    .builder(some())
                    .final_output_token(some())
                    .escrow(None)
                    .build(),
            ),
            (
                "no builder recorded",
                hint(1)
                    .builder(None)
                    .final_output_token(some())
                    .escrow(some())
                    .build(),
            ),
        ];

        for (expected, hint) in cases {
            let err = SettleBuilderFee::builder()
                .payer(payer)
                .order(order)
                .build()
                .into_atomic_group(&hint)
                .expect_err("a non-zero amount with an incomplete hint must fail");
            let msg = err.to_string();
            assert!(
                msg.contains(expected),
                "expected the error to name {expected:?}, got {msg:?}"
            );
        }
    }

    /// The claim vault is the **builder's** User Account ATA. Deriving it from
    /// the payer, who is only funding the transaction here, is the slip this
    /// pins.
    #[test]
    fn claim_vault_derives_from_the_builder_user_account() -> crate::Result<()> {
        let payer = Pubkey::new_unique();
        let order = Pubkey::new_unique();
        let builder_user = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        let settle = SettleBuilderFee::builder()
            .payer(payer)
            .order(order)
            .build();
        let program = settle.program.clone();

        let expected = get_associated_token_address_with_program_id(
            &builder_user,
            &mint,
            &anchor_spl::token::ID,
        );
        let from_payer =
            get_associated_token_address_with_program_id(&payer, &mint, &anchor_spl::token::ID);
        assert_ne!(expected, from_payer, "the two derivations must differ");

        let group = settle.into_atomic_group(
            &hint(7)
                .builder(Some(builder_user.into()))
                .final_output_token(Some(mint.into()))
                .escrow(Some(escrow.into()))
                .build(),
        )?;

        let keys = keys_of(&group, &program.id.0);
        assert!(
            keys.contains(&expected),
            "claim vault is not the builder's ATA"
        );
        assert!(
            !keys.contains(&from_payer),
            "claim vault was derived from the payer"
        );
        assert!(keys.contains(&escrow), "escrow missing");
        Ok(())
    }
}
