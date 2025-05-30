/// User account commands.
#[derive(Debug, clap::Args)]
pub struct User {}

impl super::Command for User {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let payer = ctx.client()?.payer();
        println!("{payer}");
        Ok(())
    }
}
