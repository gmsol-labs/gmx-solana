use gmsol_sdk::serde::StringPubkey;
/// Get pubkey.
#[derive(Debug, clap::Args)]
pub struct GetPubkey {}

impl super::Command for GetPubkey {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let payer = ctx.client()?.payer();
        let output = ctx.config().output();
        let out = output.display_value_with_label("payer", StringPubkey(payer))?;
        println!("{out}");
        Ok(())
    }
}
