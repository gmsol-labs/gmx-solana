/// On-chain configuration and features management.
#[derive(Debug, clap::Args)]
pub struct Configuration {}

impl super::Command for Configuration {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let payer = ctx.client()?.payer();
        println!("{payer}");
        Ok(())
    }
}
