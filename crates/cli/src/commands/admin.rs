use gmsol_sdk::ops::StoreOps;

/// Administrative commands.
#[derive(Debug, clap::Args)]
pub struct Admin {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Initialize callback authority.
    InitCallbackAuthority,
}

impl super::Command for Admin {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::InitCallbackAuthority => client
                .initialize_callback_authority()
                .into_bundle_with_options(options)?,
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
