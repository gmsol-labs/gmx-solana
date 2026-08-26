use std::{future::Future, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, transaction_builder::TransactionBuilder, IntoAtomicGroup,
};
use gmsol_utils::pubkey::optional_address;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::builders::order::{SetBuilderFee, SetBuilderFeeHint};

/// Operations for builder fees.
pub trait BuilderFeeOps<C> {
    /// Settle the builder fee of the given order.
    ///
    /// Permissionless and idempotent: safe to call on any order, in any
    /// state, whether or not it has a non-zero recorded builder fee
    /// amount.
    fn settle_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<SettleBuilderFeeHint>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Checkpoint a builder and its fee factor onto a pending order.
    ///
    /// Must be signed by the order's owner. `expected_factor` has to equal what
    /// the builder currently advertises, so read it from the builder's User
    /// Account rather than guessing; the call is rejected on any mismatch.
    ///
    /// Passing a User Account that advertises `0` clears the checkpoint, which
    /// is how a builder fee is cancelled.
    fn set_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        builder: &Pubkey,
        expected_factor: u128,
        hint: Option<SetBuilderFeeHint>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Claim the full balance of the caller's own claim vault for the
    /// given token mint, to a destination token account of their choice.
    ///
    /// Restricted to the payer's own User Account by the program: no
    /// hint is needed, since every account here is a pure function of
    /// `store`, `token_mint`, `destination`, and the payer's own key.
    /// Idempotent once the claim vault exists: safe to call whether or
    /// not it holds a non-zero balance.
    ///
    /// The claim vault is required by the program, so a builder that has
    /// never been settled for this mint fails here rather than silently
    /// succeeding, which is the more useful answer for a caller who asked
    /// to be paid.
    fn claim_builder_fees(
        &self,
        store: &Pubkey,
        token_mint: &Pubkey,
        destination: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>>;
}

/// Hint for [`settle_builder_fee`](BuilderFeeOps::settle_builder_fee), to
/// avoid an extra fetch of the order account when the caller already has
/// this data on hand.
#[derive(Debug, Clone, Copy)]
pub struct SettleBuilderFeeHint {
    /// The order's recorded builder fee amount.
    pub builder_fee_amount: u64,
    /// The builder's User Account, if the order has a builder attached.
    pub builder: Option<Pubkey>,
    /// The order's final output token mint, i.e. the token the builder
    /// fee is denominated in.
    pub final_output_token: Pubkey,
    /// The order's escrow account for the final output token.
    pub escrow: Pubkey,
}

impl<C: Deref<Target = impl Signer> + Clone> BuilderFeeOps<C> for crate::Client<C> {
    async fn settle_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<SettleBuilderFeeHint>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let hint =
            match hint {
                Some(hint) => hint,
                None => {
                    let account = self.order(order).await?;
                    let final_output_token =
                        account.tokens.final_output_token.token().ok_or_else(|| {
                            crate::Error::custom("order has no final output token")
                        })?;
                    let escrow =
                        account.tokens.final_output_token.account().ok_or_else(|| {
                            crate::Error::custom("order has no final output token")
                        })?;
                    SettleBuilderFeeHint {
                        builder_fee_amount: account.builder_fee_amount,
                        builder: optional_address(&account.builder).copied(),
                        final_output_token,
                        escrow,
                    }
                }
            };

        // The builder-related accounts are only required for a genuine
        // settlement: the no-op path performs no CPI and does not touch
        // them, so they need not even exist on-chain.
        let (builder_user, claim_vault) = if hint.builder_fee_amount == 0 {
            (None, None)
        } else {
            let builder = hint.builder.ok_or_else(|| {
                crate::Error::custom(
                    "order has a non-zero builder fee amount but no builder recorded",
                )
            })?;
            let claim_vault = get_associated_token_address_with_program_id(
                &builder,
                &hint.final_output_token,
                &anchor_spl::token::ID,
            );
            (Some(builder), Some(claim_vault))
        };

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::SettleBuilderFee {
                store: *store,
                order: *order,
                final_output_token: hint.final_output_token,
                escrow: hint.escrow,
                builder_user,
                claim_vault,
                token_program: anchor_spl::token::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::SettleBuilderFee {});

        Ok(rpc)
    }

    async fn set_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        builder: &Pubkey,
        expected_factor: u128,
        hint: Option<SetBuilderFeeHint>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let ix = SetBuilderFee::builder()
            .program(self.store_program_for_builders(store))
            .payer(self.payer())
            .order(*order)
            .builder(*builder)
            .expected_factor(expected_factor)
            .build();

        let hint = match hint {
            Some(hint) => hint,
            None => SetBuilderFeeHint::from_rpc_client_with(&ix, self.rpc()).await?,
        };

        let ag = ix.into_atomic_group(&hint)?;

        Ok(self.store_transaction().pre_atomic_group(ag, true))
    }

    fn claim_builder_fees(
        &self,
        store: &Pubkey,
        token_mint: &Pubkey,
        destination: &Pubkey,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user_account = self.find_user_address(store, &owner);
        let claim_vault = get_associated_token_address_with_program_id(
            &user_account,
            token_mint,
            &anchor_spl::token::ID,
        );
        let user_token_controller =
            self.find_user_token_controller_address(&user_account, token_mint);

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::ClaimBuilderFees {
                owner,
                store: *store,
                user_account,
                token_mint: *token_mint,
                claim_vault,
                destination: *destination,
                user_token_controller,
                token_program: anchor_spl::token::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::ClaimBuilderFees {});

        Ok(rpc)
    }
}
