use gmsol_sdk::{
    ops::user::UserOps, programs::anchor_lang::prelude::Pubkey,
    programs::gmsol_store::accounts::ReferralCodeV2,
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
        };

        let bundle = txn.into_bundle_with_options(options)?;
        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
