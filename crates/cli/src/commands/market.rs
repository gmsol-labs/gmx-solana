/// Commands for markets.
#[derive(Debug, clap::Args)]
pub struct Market {}

impl super::Command for Market {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let markets = client.markets(ctx.store()).await?;
        println!("{markets:#?}");
        Ok(())
    }
}
