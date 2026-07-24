use std::{future::Future, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::pubkey::optional_address;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

/// Operations for builder fees.
pub trait BuilderFeeOps<C> {
    /// Initialize the per-token access control account for builder fees.
    fn initialize_builder_fee_token_controller(
        &self,
        store: &Pubkey,
        token_mint: &Pubkey,
    ) -> TransactionBuilder<C, Pubkey>;

    /// Settle the builder fee of the given order.
    fn settle_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<SettleBuilderFeeHint>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Claim the settled builder fees of the given token to the given
    /// destination token account.
    ///
    /// The payer must be the owner of the builder's User Account.
    fn claim_builder_fees(
        &self,
        store: &Pubkey,
        token: &Pubkey,
        receiver_vault: &Pubkey,
    ) -> TransactionBuilder<C>;
}

/// Hint for [`settle_builder_fee`](BuilderFeeOps::settle_builder_fee).
#[derive(Debug, Clone, Copy)]
pub struct SettleBuilderFeeHint {
    /// The builder's User Account.
    ///
    /// When the order has no builder set, its builder fee amount is
    /// necessarily zero and settlement is a no-op; any initialized User
    /// Account of the store can be used.
    pub builder: Pubkey,
    /// The collateral token of the order.
    pub collateral_token: Pubkey,
    /// The order's escrow account for the collateral token.
    pub escrow: Pubkey,
}

impl<C: Deref<Target = impl Signer> + Clone> BuilderFeeOps<C> for crate::Client<C> {
    fn initialize_builder_fee_token_controller(
        &self,
        store: &Pubkey,
        token_mint: &Pubkey,
    ) -> TransactionBuilder<C, Pubkey> {
        let controller = self.find_builder_fee_token_controller_address(store, token_mint);
        self.store_transaction()
            .anchor_accounts(accounts::InitializeBuilderFeeTokenController {
                authority: self.payer(),
                store: *store,
                token_mint: *token_mint,
                controller,
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializeBuilderFeeTokenController {})
            .output(controller)
    }

    async fn settle_builder_fee(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        hint: Option<SettleBuilderFeeHint>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let hint = match hint {
            Some(hint) => hint,
            None => {
                let order = self.order(order).await?;
                let builder = match optional_address(&order.builder) {
                    Some(builder) => *builder,
                    // The order has no builder, so its builder fee amount is
                    // necessarily zero and settlement is a no-op. The payer's
                    // own User Account is used as a placeholder.
                    None => self.find_user_address(store, &self.payer()),
                };
                let collateral_token = order.params.collateral_token;
                let escrow = [&order.tokens.long_token, &order.tokens.short_token]
                    .into_iter()
                    .filter_map(|t| t.token_and_account())
                    .find_map(|(token, account)| (token == collateral_token).then_some(account))
                    .ok_or_else(|| {
                        crate::Error::custom("no escrow account for the collateral token")
                    })?;
                SettleBuilderFeeHint {
                    builder,
                    collateral_token,
                    escrow,
                }
            }
        };

        let claim_vault = get_associated_token_address_with_program_id(
            &hint.builder,
            &hint.collateral_token,
            &anchor_spl::token::ID,
        );

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::SettleBuilderFee {
                store: *store,
                order: *order,
                builder_user: hint.builder,
                collateral_token: hint.collateral_token,
                escrow: hint.escrow,
                claim_vault,
                token_program: anchor_spl::token::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::SettleBuilderFee {});

        Ok(rpc)
    }

    fn claim_builder_fees(
        &self,
        store: &Pubkey,
        token: &Pubkey,
        receiver_vault: &Pubkey,
    ) -> TransactionBuilder<C> {
        let owner = self.payer();
        let builder_user = self.find_user_address(store, &owner);
        let claim_vault = get_associated_token_address_with_program_id(
            &builder_user,
            token,
            &anchor_spl::token::ID,
        );
        self.store_transaction()
            .anchor_accounts(accounts::ClaimBuilderFees {
                owner,
                store: *store,
                builder_user,
                controller: self.find_builder_fee_token_controller_address(store, token),
                token: *token,
                claim_vault,
                receiver_vault: *receiver_vault,
                token_program: anchor_spl::token::ID,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::ClaimBuilderFees {})
    }
}
