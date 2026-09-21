use anchor_spl::associated_token::get_associated_token_address;
use gmsol_sdk::{
    ops::{builder_fee::BuilderFeeOps, token_account::TokenAccountOps, user::UserOps},
    programs::anchor_lang::prelude::Pubkey,
    programs::gmsol_store::accounts::ReferralCodeV2,
    utils::Value,
};

/// User account commands.
#[derive(Debug, clap::Args)]
pub struct User {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Prepare User Account.
    Prepare,
    /// Initialize Referral Code.
    InitReferralCode { code: String },
    /// Transfer Referral Code.
    TransferReferralCode { receiver: Pubkey },
    /// Cancel referral code transfer.
    CancelReferralCodeTransfer,
    /// Accept referral code transfer.
    AcceptReferralCode { code: String },
    /// Set Referrer.
    SetReferrer { code: String },
    /// Set the builder fee for a pending order. Must be signed by the order's owner.
    ///
    /// To detach a builder, pass a User Account that advertises `0` with
    /// `--expected-factor 0`. Your own account does if you never set a factor,
    /// and that is the only way to clear a checkpoint.
    SetBuilderFee {
        /// The order to attach the builder fee to.
        order: Pubkey,
        /// The builder receiving the fee.
        #[arg(long)]
        builder: Pubkey,
        /// Expected fee factor, in decimal notation (`0.001` is 0.1%), not the raw
        /// on-chain integer. Must equal what `builder`'s User Account currently
        /// advertises; the call is rejected on any mismatch.
        #[arg(long)]
        expected_factor: Value,
    },
    /// Set the builder fee factor advertised by the payer's own User Account.
    SetBuilderFeeFactor {
        /// New factor, in decimal notation (`0.001` is 0.1%), not the raw on-chain
        /// integer. Must not exceed the store's `MaxBuilderFeeFactor`, which reads
        /// `0` until a config keeper raises it. Pass 0 to opt out.
        factor: Value,
    },
    /// Claim the payer's accrued builder fees for a token mint.
    ClaimBuilderFees {
        /// Token mint to claim.
        token_mint: Pubkey,
        /// Destination token account to receive the claimed amount. Defaults to the
        /// payer's associated token account for `TOKEN_MINT`, which is created if it
        /// does not exist yet. An explicitly passed account is never created.
        #[arg(long)]
        destination: Option<Pubkey>,
    },
}

impl super::Command for User {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let txn = match &self.command {
            Command::Prepare => client.prepare_user(store)?,
            Command::InitReferralCode { code } => {
                client.initialize_referral_code(store, ReferralCodeV2::decode(code)?)?
            }
            Command::TransferReferralCode { receiver } => {
                client.transfer_referral_code(store, receiver, None).await?
            }
            Command::CancelReferralCodeTransfer => {
                client.cancel_referral_code_transfer(store, None).await?
            }
            Command::AcceptReferralCode { code } => {
                client
                    .accept_referral_code(store, ReferralCodeV2::decode(code)?, None)
                    .await?
            }
            Command::SetReferrer { code } => {
                client
                    .set_referrer(store, ReferralCodeV2::decode(code)?, None)
                    .await?
            }
            Command::SetBuilderFee {
                order,
                builder,
                expected_factor,
            } => {
                client
                    .set_builder_fee(store, order, builder, expected_factor.to_u128()?, None)
                    .await?
            }
            Command::SetBuilderFeeFactor { factor } => {
                client.set_builder_fee_factor(store, factor.to_u128()?)?
            }
            Command::ClaimBuilderFees {
                token_mint,
                destination,
            } => match destination {
                Some(destination) => client.claim_builder_fees(store, token_mint, destination)?,
                None => {
                    // The claim vault is an associated token account of the legacy
                    // token program, so the mint is one too and the default
                    // destination derives unambiguously.
                    let destination = get_associated_token_address(&client.payer(), token_mint);
                    let prepare = client.prepare_associated_token_account(
                        token_mint,
                        &anchor_spl::token::ID,
                        None,
                    );
                    prepare.merge(client.claim_builder_fees(store, token_mint, &destination)?)
                }
            },
        };

        let bundle = txn.into_bundle_with_options(options)?;
        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
