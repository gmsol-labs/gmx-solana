use gmsol_sdk::{
    ops::{builder_fee::BuilderFeeOps, user::UserOps},
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
    SetBuilderFee {
        /// The order to attach the builder fee to.
        order: Pubkey,
        /// The builder receiving the fee.
        #[arg(long)]
        builder: Pubkey,
        /// Expected fee factor, must equal what `builder`'s User Account currently
        /// advertises; the call is rejected on any mismatch.
        #[arg(long)]
        expected_factor: Value,
    },
    /// Set the builder fee factor advertised by the payer's own User Account.
    SetBuilderFeeFactor {
        /// New factor. Must not exceed the store's configured maximum. Pass 0 to opt out.
        factor: Value,
    },
    /// Claim the payer's accrued builder fees for a token mint.
    ClaimBuilderFees {
        /// Token mint to claim.
        token_mint: Pubkey,
        /// Destination token account to receive the claimed amount.
        #[arg(long)]
        destination: Pubkey,
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
            } => client.claim_builder_fees(store, token_mint, destination)?,
        };

        let bundle = txn.into_bundle_with_options(options)?;
        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
