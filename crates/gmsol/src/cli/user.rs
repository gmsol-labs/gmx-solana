use gmsol::{
    store::user::UserOps, types::user::ReferralCodeV2, utils::instruction::InstructionSerialization,
};
use gmsol_solana_utils::bundle_builder::BundleOptions;
use solana_sdk::pubkey::Pubkey;

use crate::{GMSOLClient, InstructionBufferCtx};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
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

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        let options = BundleOptions {
            max_packet_size: max_transaction_size,
            ..Default::default()
        };

        let bundle = match &self.command {
            Command::Prepare => client
                .prepare_user(store)?
                .into_bundle_with_options(options)?,
            Command::InitReferralCode { code } => client
                .initialize_referral_code(store, ReferralCodeV2::decode(code)?)?
                .into_bundle_with_options(options)?,
            Command::TransferReferralCode { receiver } => client
                .transfer_referral_code(store, receiver, None)
                .await?
                .into_bundle_with_options(options)?,
            Command::CancelReferralCodeTransfer => client
                .cancel_referral_code_transfer(store, None)
                .await?
                .into_bundle_with_options(options)?,
            Command::AcceptReferralCode { code } => client
                .accept_referral_code(store, ReferralCodeV2::decode(code)?, None)
                .await?
                .into_bundle_with_options(options)?,
            Command::SetReferrer { code } => client
                .set_referrer(store, ReferralCodeV2::decode(code)?, None)
                .await?
                .into_bundle_with_options(options)?,
        };

        crate::utils::send_or_serialize_bundle_with_default_callback(
            store,
            bundle,
            ctx,
            serialize_only,
            skip_preflight,
        )
        .await
    }
}
