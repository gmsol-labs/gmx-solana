use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{
    exchange::ExchangeOps,
    types::feature::{display_feature, ActionDisabledFlag, DomainDisabledFlag},
    utils::instruction::InstructionSerialization,
};

use crate::GMSOLClient;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Toggle feature.
    Toggle {
        /// Feature domain.
        #[clap(requires = "toggle")]
        domain: DomainDisabledFlag,
        /// Feature action.
        action: ActionDisabledFlag,
        /// Enable the given feature.
        #[arg(long, group = "toggle")]
        enable: bool,
        /// Disable the given feature.
        #[arg(long, group = "toggle")]
        disable: bool,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: Option<InstructionSerialization>,
        priority_lamports: u64,
    ) -> gmsol::Result<()> {
        match self.command {
            Command::Toggle {
                domain,
                action,
                enable,
                disable,
            } => {
                if enable == disable {
                    return Err(gmsol::Error::invalid_argument("invalid toggle flags"));
                }
                let req = client.toggle_feature(store, domain, action, enable);
                crate::utils::send_or_serialize_transaction(
                    store,
                    req,
                    None,
                    serialize_only,
                    false,
                    Some(priority_lamports),
                    |signature| {
                        let msg = if enable { "enabled" } else { "disabled" };
                        tracing::info!("{msg} feature: {}", display_feature(domain, action));
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
        }
    }
}
