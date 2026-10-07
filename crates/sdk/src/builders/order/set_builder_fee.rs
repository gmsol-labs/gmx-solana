use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, AtomicGroup, IntoAtomicGroup, ProgramExt,
};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

/// Builder for the `set_builder_fee` instruction.
///
/// Checkpoints a builder and its fee factor onto a pending order. The order's
/// owner must sign, and the factor must match what the builder currently
/// advertises, so the caller has to read that factor before building this
/// instruction rather than letting the program pick it up implicitly.
///
/// Submit this instruction with order creation in the same transaction when a
/// builder fee is required. If submitted separately, a keeper can execute the
/// order without the fee before this checkpoint lands. Execution or closure
/// prevents attaching it afterwards. Check the order's state before retrying a
/// failed checkpoint, since a non-pending order will reject it again.
///
/// A standalone checkpoint binds itself to one order instance through the
/// order ID carried by [`SetBuilderFeeHint`], so build the hint from a
/// fresh read of the order account.
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
///
/// Build it from a fresh read of the order account. The order ID it
/// carries is what binds the checkpoint to the order instance that was read:
/// order addresses are reused once the occupying order closes, so a hint read
/// from a previous instance makes the program reject the checkpoint rather
/// than attach a fee to a different order at the same address. Only the
/// same-transaction create flow cannot know the ID at build time; it passes
/// a placeholder instead and is accepted after the transaction's creation
/// instruction for that order.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetBuilderFeeHint {
    /// Market recorded on the order.
    #[builder(setter(into))]
    pub market: StringPubkey,
    /// The order's final output token.
    ///
    /// Read from the order account. An order whose final output token is
    /// uninitialized cannot carry a builder fee, so there is no hint to build
    /// here and the instruction would be rejected anyway.
    #[builder(setter(into))]
    pub final_output_token: StringPubkey,
    /// The order's ID, read from the same fresh read of the order
    /// account. See the hint's own documentation for what it binds and when a
    /// placeholder is accepted.
    pub order_id: u64,
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

        // A zero-factor checkpoint revokes the fee, so it needs no claim vault.
        // A nonzero checkpoint still requires the existing builder ATA.
        let claim_vault: Option<Pubkey> = (self.expected_factor != 0).then(|| {
            get_associated_token_address_with_program_id(
                &builder,
                &final_output_token,
                &token_program_id,
            )
        });

        let set = self
            .program
            .anchor_instruction(args::SetBuilderFee {
                expected_factor: self.expected_factor,
                expected_order_id: hint.order_id,
            })
            .anchor_accounts(
                accounts::SetBuilderFee {
                    owner: payer,
                    store: self.program.store.0,
                    market: hint.market.0,
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
            .accounts(vec![AccountMeta::new_readonly(
                solana_sdk::sysvar::instructions::ID,
                false,
            )])
            .build();

        Ok(AtomicGroup::with_instructions(&payer, Some(set)))
    }
}

impl FromRpcClientWith<SetBuilderFee> for SetBuilderFeeHint {
    async fn from_rpc_client_with<'a>(
        builder: &'a SetBuilderFee,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        use crate::{programs::gmsol_store::accounts::Order, utils::zero_copy::ZeroCopy};
        use gmsol_solana_utils::client_traits::RpcClientExt;

        let order = client
            .get_anchor_account::<ZeroCopy<Order>>(&builder.order.0, Default::default())
            .await?
            .0;

        // An uninitialized final output token is the one failure worth naming
        // here rather than letting the program reject it: it means the order
        // was created without a fee output slot, which no amount of retrying
        // fixes.
        let final_output_token = order.tokens.final_output_token.token().ok_or_else(|| {
            gmsol_solana_utils::Error::custom(
                "the order's final output token is uninitialized, so it cannot carry a builder fee",
            )
        })?;

        Ok(Self {
            market: order.header.market.into(),
            final_output_token: final_output_token.into(),
            order_id: order.header.id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_factor_uses_the_optional_account_sentinel() -> crate::Result<()> {
        let scub_owner = Pubkey::new_unique();
        let scub_order = Pubkey::new_unique();
        let scub_builder = Pubkey::new_unique();
        let scub_market = Pubkey::new_unique();
        let scub_mint = Pubkey::new_unique();
        for (factor, order_id) in [(0, 0u64), (1, 42), (1, 0)] {
            let hint = SetBuilderFeeHint::builder()
                .market(scub_market)
                .final_output_token(scub_mint)
                .order_id(order_id)
                .build();
            let expected_vault = (factor != 0).then(|| {
                get_associated_token_address_with_program_id(
                    &scub_builder,
                    &scub_mint,
                    &anchor_spl::token::ID,
                )
            });
            let builder = SetBuilderFee::builder()
                .payer(scub_owner)
                .order(scub_order)
                .builder(scub_builder)
                .expected_factor(factor)
                .build();
            let program_id = builder.program.id.0;
            let group = builder.into_atomic_group(&hint)?;
            let instruction = group
                .instructions_with_options(Default::default())
                .find(|ix| ix.program_id == program_id)
                .expect("set_builder_fee instruction must be present");

            assert_eq!(
                instruction.accounts.last().unwrap().pubkey,
                solana_sdk::sysvar::instructions::ID,
            );

            assert_eq!(
                instruction.accounts[6].pubkey,
                expected_vault.unwrap_or(program_id),
                "claim_vault account differs from the wire format for factor {factor}"
            );

            // After the 8-byte discriminator the data is `expected_factor`
            // followed by `expected_order_id`, both borsh-encoded.
            let args = &instruction.data[8..];
            assert_eq!(
                args[..16],
                factor.to_le_bytes(),
                "expected_factor differs from the wire format for factor {factor}"
            );
            assert_eq!(
                args[16..],
                order_id.to_le_bytes(),
                "expected_order_id differs from the wire format for order {order_id}"
            );
        }

        Ok(())
    }
}
