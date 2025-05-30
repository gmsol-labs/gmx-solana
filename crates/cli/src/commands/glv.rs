/// GLV management commands.
#[derive(Debug, clap::Args)]
pub struct Glv {}

impl super::Command for Glv {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let payer = ctx.client()?.payer();
        println!("{payer}");
        Ok(())
    }
}
