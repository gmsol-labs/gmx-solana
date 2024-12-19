use crate::GMSOLClient;
use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::timelock::TimelockOps;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Initialize timelock config.
    InitConfig {
        #[arg(long, default_value_t = 86400)]
        initial_delay: u32,
    },
    /// Increase timelock delay.
    IncreaseDelay { delta: u32 },
    /// Init executor.
    InitExecutor { role: String },
    /// Approve a timelocked instruction.
    Approve {
        buffer: Pubkey,
        #[arg(long)]
        role: Option<String>,
    },
    /// Cancel a timelocked instruction.
    Cancel {
        buffer: Pubkey,
        #[arg(long)]
        executor: Option<Pubkey>,
    },
    /// Execute a timelocked instruction.
    Execute { buffer: Pubkey },
    /// Revoke role.
    RevokeRole {
        /// User.
        authority: Pubkey,
        /// Role.
        role: String,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
        skip_preflight: bool,
    ) -> gmsol::Result<()> {
        let req = match &self.command {
            Command::InitConfig { initial_delay } => {
                let (rpc, config) = client
                    .initialize_timelock_config(store, *initial_delay)
                    .swap_output(());
                println!("{config}");
                rpc
            }
            Command::IncreaseDelay { delta } => client.increase_timelock_delay(store, *delta),
            Command::InitExecutor { role } => {
                let (rpc, executor) = client.initialize_executor(store, role)?.swap_output(());
                println!("{executor}");
                rpc
            }
            Command::Approve { buffer, role } => {
                client
                    .approve_timelocked_instruction(
                        store,
                        buffer,
                        role.as_ref().map(|s| s.as_str()),
                    )
                    .await?
            }
            Command::Cancel { buffer, executor } => {
                client
                    .cancel_timelocked_instruction(store, buffer, executor.as_ref())
                    .await?
            }
            Command::Execute { buffer } => {
                client
                    .execute_timelocked_instruction(store, buffer, None)
                    .await?
            }
            Command::RevokeRole { authority, role } => {
                client.timelock_bypassed_revoke_role(store, role, authority)
            }
        };
        crate::utils::send_or_serialize_rpc(
            store,
            client,
            req,
            None,
            serialize_only,
            skip_preflight,
            |signature| {
                tracing::info!("{signature}");
                Ok(())
            },
        )
        .await
    }
}
