use std::{future::Future, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, transaction_builder::TransactionBuilder, IntoAtomicGroup,
};
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::builders::order::{
    SetBuilderFee, SetBuilderFeeHint, SettleBuilderFee, SettleBuilderFeeHint,
};

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

impl<C: Deref<Target = impl Signer> + Clone> BuilderFeeOps<C> for crate::Client<C> {
    async fn settle_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<SettleBuilderFeeHint>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let ix = SettleBuilderFee::builder()
            .program(self.store_program_for_builders(store))
            .payer(self.payer())
            .order(*order)
            .build();

        let hint = match hint {
            Some(hint) => hint,
            None => SettleBuilderFeeHint::from_rpc_client_with(&ix, self.rpc()).await?,
        };

        let ag = ix.into_atomic_group(&hint)?;

        Ok(self.store_transaction().pre_atomic_group(ag, true))
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
