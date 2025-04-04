use gmsol::{
    migration::MigrationOps,
    types::{user::ReferralCodeV2, UserHeader},
    utils::ZeroCopy,
};
use gmsol_solana_utils::bundle_builder::BundleOptions;
use gmsol_store::instructions::ReferralCode;
use solana_sdk::pubkey::Pubkey;

use crate::{GMSOLClient, InstructionBufferCtx, InstructionSerialization};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Referral Code.
    ReferralCode {
        address: Vec<Pubkey>,
        #[arg(long, group = "account-kind")]
        users: bool,
        #[arg(long, group = "account-kind")]
        owners: bool,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        instruction_buffer: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        priority_lamports: u64,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        let bundle = match &self.command {
            Command::ReferralCode {
                address,
                users,
                owners,
            } => {
                let mut users = *users;
                let addresses = if *owners {
                    users = true;
                    address
                        .iter()
                        .map(|owner| client.find_user_address(store, owner))
                        .collect()
                } else {
                    address.clone()
                };
                let mut bundle = client.bundle_with_options(BundleOptions {
                    max_packet_size: max_transaction_size,
                    ..Default::default()
                });
                for address in addresses {
                    let code = if users {
                        let Some(address) = client
                            .account::<ZeroCopy<UserHeader>>(&address)
                            .await?
                            .and_then(|user| user.0.referral().code().copied())
                        else {
                            tracing::info!(user=%address, "the user account is not found, or the referral code is not set");
                            continue;
                        };

                        match client.account::<ZeroCopy<ReferralCode>>(&address).await {
                            Ok(Some(code)) => {
                                let code = ReferralCodeV2::encode(&code.0.code, true);
                                tracing::info!(%code, %address, "found legacy referral code account");
                            }
                            Ok(None) => {
                                tracing::warn!(%address, "the referral code account does not exist");
                                continue;
                            }
                            Err(err) => {
                                tracing::warn!(%address, "fetch referral code account error: {err}");
                                continue;
                            }
                        }

                        address
                    } else {
                        address
                    };
                    bundle.push(client.migrate_referral_code(store, &code))?;
                }
                bundle
            }
        };

        crate::utils::send_or_serialize_bundle_with_default_callback(
            store,
            bundle,
            instruction_buffer,
            serialize_only,
            skip_preflight,
            Some(priority_lamports),
        )
        .await?;
        Ok(())
    }
}
