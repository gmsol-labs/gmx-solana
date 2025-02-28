use gmsol::migration::MigrationOps;
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
    ReferralCode { code: Pubkey },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        instruction_buffer: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
    ) -> gmsol::Result<()> {
        let tx = match &self.command {
            Command::ReferralCode { code } => client.migrate_referral_code(store, code),
        };

        crate::utils::send_or_serialize_transaction(
            store,
            tx,
            instruction_buffer,
            serialize_only,
            skip_preflight,
            |signature| {
                tracing::info!(%signature, "migrated");
                Ok(())
            },
        )
        .await?;
        Ok(())
    }
}
