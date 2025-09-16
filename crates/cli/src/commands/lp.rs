use gmsol_sdk::{programs::anchor_lang::prelude::Pubkey, utils::Value};

/// Liquidity Provider management commands.
#[derive(Debug, clap::Args)]
pub struct Lp {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Initialize LP staking program.
    InitLp {
        /// Minimum stake value.
        #[arg(long)]
        min_stake_value: Value,
        /// Initial APY.
        #[arg(long)]
        initial_apy: Value,
    },
}

impl super::Command for Lp {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let _store = ctx.store();
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::InitLp {
                min_stake_value,
                initial_apy,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Use a placeholder for gt_mint since it's not actually used in the program logic
                let placeholder_gt_mint = Pubkey::default();
                client
                    .initialize_lp(
                        &placeholder_gt_mint,
                        min_stake_value.to_u128()?,
                        initial_apy.to_u128()?,
                    )?
                    .into_bundle_with_options(options)?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
