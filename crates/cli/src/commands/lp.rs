use gmsol_sdk::utils::Value;

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
    /// Create LP token controller for a specific token mint.
    CreateController {
        /// LP token mint address.
        lp_token_mint: Pubkey,
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

                client
                    .initialize_lp(min_stake_value.to_u128()?, initial_apy.to_u128()?)?
                    .into_bundle_with_options(options)?
            }
            Command::CreateController { lp_token_mint } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                client
                    .create_lp_token_controller(lp_token_mint)?
                    .into_bundle_with_options(options)?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
