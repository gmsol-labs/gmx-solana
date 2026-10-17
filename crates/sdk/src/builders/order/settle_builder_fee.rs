use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

const ERR_NO_MINT: &str = "the settlement accounts are required (a builder fee is recorded or may accrue) but the order has no final output token mint";
const ERR_NO_ESCROW: &str = "the settlement accounts are required (a builder fee is recorded or may accrue) but the order has no final output token escrow";
const ERR_NO_BUILDER: &str = "the settlement accounts are required (a builder fee is recorded or may accrue) but the order has no builder recorded";

/// Builder for the `settle_builder_fee` instruction.
///
/// Moves an order's recorded builder fee out of the final output token escrow
/// and into the builder's claim vault. Permissionless and idempotent: safe to
/// call on any order, in any state, whether or not it carries a non-zero
/// recorded amount.
///
/// The instruction is safe to build **before** the fee exists. When the order
/// has a builder that can still be charged, the settlement accounts are
/// attached whatever amount the order records now, so a settlement placed
/// after an execution in the same transaction finds the accounts it needs
/// once that execution has recorded the fee. See [`SettleBuilderFeeHint`] for
/// the exact rule.
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
/// Every field is read off the order account.
///
/// # Which accounts are attached
///
/// The four settlement accounts (final output token, escrow, builder user and
/// claim vault) are attached when a settlement may have to transfer:
///
/// - the order records a non-zero `builder_fee_amount`, or
/// - the order has a `builder` and its `builder_fee_factor` is not known to be
///   zero, which means an execution can still record a fee before this
///   instruction runs.
///
/// Otherwise they are left out, and the instruction is the no-op the program
/// accepts for an order that owes nothing. That covers an order that never had
/// a builder, and a revoked one, whose `builder` stays set but whose factor is
/// zero: such a builder may have no claim vault, and the program validates
/// every account it is handed, so sending one would fail a settlement that has
/// nothing to settle.
///
/// # Freshness
///
/// Only `builder_fee_amount` changes once an order has left the pending state.
/// `builder`, `builder_fee_factor`, `final_output_token` and `escrow` are fixed
/// from then on, which is why the rule above does not depend on the amount
/// being current: a hint read from a pending order stays correct across an
/// execution placed before the settlement in the same transaction.
///
/// What the read cannot cover is a change made *to those fixed fields* after
/// it. An owner can still call `set_builder_fee` while the order is pending, so
/// a hint taken before a builder was checkpointed (`builder: None`), or before a
/// revoked order was re-checkpointed, leaves the accounts out and then fails
/// with `TokenMintNotProvided` if a fee is charged. A stale `escrow` fails with
/// `TokenAccountMismatched` and a stale `builder` with `InvalidUserAccount`.
/// `FromRpcClientWith<SettleBuilderFee>` derives every field from the live
/// account; prefer it to a hint cached across a user session.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SettleBuilderFeeHint {
    /// The order's recorded builder fee amount.
    pub builder_fee_amount: u64,
    /// The builder fee factor checkpointed onto the order.
    ///
    /// `Some(0)` marks a revoked checkpoint, which cannot be charged. `None`
    /// means the factor is not known, and is treated as chargeable so that a
    /// settlement built from a hint without it still carries its accounts.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub builder_fee_factor: Option<u128>,
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

impl SettleBuilderFeeHint {
    /// Whether the settlement may have to transfer, and so needs its accounts.
    ///
    /// True once a fee is recorded, and also while one can still be: an order
    /// with a builder whose checkpointed factor is not zero is charged by its
    /// execution, which can run in the same transaction ahead of this
    /// instruction. A revoked checkpoint (`Some(0)`) keeps its `builder` but is
    /// never charged, and its claim vault need not exist, so it stays on the
    /// account-less path.
    fn may_transfer(&self) -> bool {
        self.builder_fee_amount != 0
            || (self.builder.is_some() && self.builder_fee_factor != Some(0))
    }
}

impl IntoAtomicGroup for SettleBuilderFee {
    type Hint = SettleBuilderFeeHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;

        // All four accounts are required only for a settlement that may
        // transfer. On the no-op path they need not even exist on-chain, so an
        // order that owes nothing must not be turned into an error: refusing to
        // build the instruction is what would make the no-op unreachable for
        // exactly those orders.
        //
        // The amount alone cannot decide this. It is read before the execution
        // that records the fee, and this instruction may land after it in the
        // same transaction, so a zero here says nothing about a zero at
        // landing. See `may_transfer`.
        //
        // Each missing one names itself. A hint derived from the order cannot
        // miss the mint without the escrow, since `TokenAndAccount` records
        // both or neither, but a caller-supplied hint can.
        let (final_output_token, escrow, builder_user, claim_vault) = if !hint.may_transfer() {
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
            builder_fee_factor: Some(order.builder_fee_factor),
            builder: optional_address(&order.builder).copied().map(Into::into),
            final_output_token: order.tokens.final_output_token.token().map(Into::into),
            escrow: order.tokens.final_output_token.account().map(Into::into),
        })
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

    use super::*;

    fn hint(amount: u64) -> SettleBuilderFeeHintBuilder<((u64,), (), (), (), ())> {
        SettleBuilderFeeHint::builder().builder_fee_amount(amount)
    }

    fn keys_of(group: &AtomicGroup, program_id: &Pubkey) -> Vec<Pubkey> {
        group
            .instructions_with_options(Default::default())
            .filter(|ix| ix.program_id == *program_id)
            .flat_map(|ix| ix.accounts.iter().map(|m| m.pubkey).collect::<Vec<_>>())
            .collect()
    }

    /// The instruction addressed to `program_id`, whole.
    fn program_instruction(group: &AtomicGroup, program_id: &Pubkey) -> Instruction {
        group
            .instructions_with_options(Default::default())
            .map(|ix| ix.into_owned())
            .find(|ix| ix.program_id == *program_id)
            .expect("the settlement instruction must be present")
    }

    /// The accounts a settlement has to carry once it may transfer.
    struct SettlementAccounts {
        builder_user: Pubkey,
        mint: Pubkey,
        escrow: Pubkey,
        claim_vault: Pubkey,
    }

    impl SettlementAccounts {
        fn new() -> Self {
            let builder_user = Pubkey::new_unique();
            let mint = Pubkey::new_unique();
            Self {
                builder_user,
                mint,
                escrow: Pubkey::new_unique(),
                claim_vault: get_associated_token_address_with_program_id(
                    &builder_user,
                    &mint,
                    &anchor_spl::token::ID,
                ),
            }
        }

        /// A hint shaped like the one `FromRpcClientWith` derives from an
        /// order that has a builder attached.
        fn hint_with_builder(&self, amount: u64, factor: Option<u128>) -> SettleBuilderFeeHint {
            hint(amount)
                .builder_fee_factor(factor)
                .builder(Some(self.builder_user.into()))
                .final_output_token(Some(self.mint.into()))
                .escrow(Some(self.escrow.into()))
                .build()
        }

        fn assert_attached(&self, keys: &[Pubkey]) {
            for (name, key) in [
                ("builder user", self.builder_user),
                ("mint", self.mint),
                ("escrow", self.escrow),
                ("claim vault", self.claim_vault),
            ] {
                assert!(keys.contains(&key), "{name} must be attached");
            }
        }

        fn assert_left_out(&self, keys: &[Pubkey]) {
            for (name, key) in [
                ("builder user", self.builder_user),
                ("mint", self.mint),
                ("escrow", self.escrow),
                ("claim vault", self.claim_vault),
            ] {
                assert!(!keys.contains(&key), "{name} must not be attached");
            }
        }
    }

    /// An order that never had a builder can never be charged, and a zero
    /// recorded amount is then a supported state, not an error. The no-op
    /// performs no CPI, so the four settlement accounts must stay out of the
    /// instruction even when the caller hands over a hint that carries a mint
    /// and an escrow.
    #[test]
    fn no_builder_keeps_the_account_less_path() -> crate::Result<()> {
        let payer = Pubkey::new_unique();
        let order = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let escrow = Pubkey::new_unique();

        let settle = SettleBuilderFee::builder()
            .payer(payer)
            .order(order)
            .build();
        let program = settle.program.clone();

        let group = settle.into_atomic_group(
            &hint(0)
                .builder(None)
                .final_output_token(Some(mint.into()))
                .escrow(Some(escrow.into()))
                .build(),
        )?;

        let keys = keys_of(&group, &program.id.0);
        assert!(keys.contains(&order), "the order account is always present");
        for (name, key) in [("mint", mint), ("escrow", escrow)] {
            assert!(
                !keys.contains(&key),
                "{name} must not be attached when no builder can be charged"
            );
        }
        Ok(())
    }

    /// The fee is recorded by the execution that runs before this
    /// instruction lands, so a hint read from a still-pending order carries a
    /// zero amount. With a builder attached the accounts have to be there
    /// regardless, or the instruction reverts with `TokenMintNotProvided` once
    /// the amount turns non-zero, and takes the execution down with it.
    #[test]
    fn a_builder_attaches_the_settlement_accounts_at_a_zero_amount() -> crate::Result<()> {
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

        let claim_vault = get_associated_token_address_with_program_id(
            &builder_user,
            &mint,
            &anchor_spl::token::ID,
        );
        let keys = keys_of(&group, &program.id.0);
        for (name, key) in [
            ("builder user", builder_user),
            ("mint", mint),
            ("escrow", escrow),
            ("claim vault", claim_vault),
        ] {
            assert!(keys.contains(&key), "{name} must be attached");
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

    fn settle_group(hint: &SettleBuilderFeeHint) -> crate::Result<(AtomicGroup, Pubkey, Pubkey)> {
        let order = Pubkey::new_unique();
        let settle = SettleBuilderFee::builder()
            .payer(Pubkey::new_unique())
            .order(order)
            .build();
        let program_id = settle.program.id.0;
        Ok((settle.into_atomic_group(hint)?, program_id, order))
    }

    /// A revoked checkpoint keeps its `builder` but is never charged, and its
    /// builder may have no claim vault: a zero-factor checkpoint does not
    /// require one. The program validates every account it is handed, so
    /// attaching a vault that does not exist would fail a settlement that has
    /// nothing to settle, and a close grouped with it.
    #[test]
    fn a_revoked_checkpoint_keeps_the_account_less_path() -> crate::Result<()> {
        let accounts = SettlementAccounts::new();
        let (group, program_id, order) = settle_group(&accounts.hint_with_builder(0, Some(0)))?;

        let keys = keys_of(&group, &program_id);
        assert!(keys.contains(&order), "the order account is always present");
        accounts.assert_left_out(&keys);
        Ok(())
    }

    /// A fee that is already recorded is settled whatever the factor says: the
    /// amount is what the program reads at landing, so it wins.
    #[test]
    fn a_recorded_amount_attaches_the_accounts_whatever_the_factor() -> crate::Result<()> {
        let accounts = SettlementAccounts::new();
        for factor in [None, Some(0), Some(1)] {
            let (group, program_id, _) = settle_group(&accounts.hint_with_builder(7, factor))?;
            accounts.assert_attached(&keys_of(&group, &program_id));
        }
        Ok(())
    }

    /// A checkpointed non-zero factor is what makes an execution charge, so it
    /// is enough on its own, and so is a hint that does not know the factor.
    #[test]
    fn a_chargeable_builder_attaches_the_accounts_at_a_zero_amount() -> crate::Result<()> {
        let accounts = SettlementAccounts::new();
        for factor in [Some(1), Some(u128::MAX), None] {
            let (group, program_id, _) = settle_group(&accounts.hint_with_builder(0, factor))?;
            accounts.assert_attached(&keys_of(&group, &program_id));
        }
        Ok(())
    }

    /// The instruction must not depend on the amount once a builder can be
    /// charged. This is the property the atomic execute-and-settle case rests
    /// on: the instruction built from a pending order (amount zero) has to be
    /// the one that would have been built after the fee was recorded.
    #[test]
    fn the_instruction_does_not_depend_on_the_amount_for_a_chargeable_builder() -> crate::Result<()>
    {
        let accounts = SettlementAccounts::new();
        let order = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let build = |amount: u64| -> crate::Result<(Instruction, AtomicGroup)> {
            let settle = SettleBuilderFee::builder()
                .payer(payer)
                .order(order)
                .build();
            let program_id = settle.program.id.0;
            let group = settle.into_atomic_group(&accounts.hint_with_builder(amount, Some(1)))?;
            Ok((program_instruction(&group, &program_id), group))
        };

        let (before_execution, _) = build(0)?;
        let (after_execution, _) = build(7)?;
        assert_eq!(
            before_execution, after_execution,
            "the instruction built before the fee exists must equal the one built after"
        );
        Ok(())
    }

    /// With a builder that may be charged, a hint that lacks what settlement
    /// needs is a caller error even at a zero amount, and names what is
    /// missing. Leaving the account out instead would revert the transaction
    /// the moment the fee is recorded.
    #[test]
    fn a_missing_field_is_an_error_at_a_zero_amount_when_a_builder_can_be_charged() {
        let some = || Some(StringPubkey::from(Pubkey::new_unique()));
        let cases = [
            (
                "no final output token mint",
                hint(0)
                    .builder(some())
                    .final_output_token(None)
                    .escrow(some())
                    .build(),
            ),
            (
                "no final output token escrow",
                hint(0)
                    .builder(some())
                    .final_output_token(some())
                    .escrow(None)
                    .build(),
            ),
        ];

        for (expected, hint) in cases {
            let err = settle_group(&hint).expect_err("an incomplete hint must fail");
            let msg = err.to_string();
            assert!(
                msg.contains(expected),
                "expected the error to name {expected:?}, got {msg:?}"
            );
        }
    }

    /// A recorded amount with no builder to pay is not a state the program
    /// can produce, so a hint saying so is rejected rather than settled blind.
    #[test]
    fn a_recorded_amount_without_a_builder_is_an_error() {
        let hint = hint(5)
            .builder(None)
            .final_output_token(Some(Pubkey::new_unique().into()))
            .escrow(Some(Pubkey::new_unique().into()))
            .build();

        let err = settle_group(&hint).expect_err("an amount without a builder must fail");
        assert!(err.to_string().contains("no builder recorded"), "got {err}");
    }
}
