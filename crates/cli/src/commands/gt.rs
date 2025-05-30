/// GT-related commands.
#[derive(Debug, clap::Args)]
pub struct Gt {}

impl super::Command for Gt {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let payer = ctx.client()?.payer();
        println!("{payer}");
        Ok(())
    }
}
