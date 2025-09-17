use gmsol_sdk::{
    builders::liquidity_provider::LpTokenKind, programs::anchor_lang::prelude::Pubkey,
    solana_utils::make_bundle_builder::MakeBundleBuilder, utils::Value,
};
use std::num::NonZeroU64;

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
    /// Disable LP token controller for a specific token mint.
    DisableController {
        /// LP token mint address.
        lp_token_mint: Pubkey,
    },
    /// Stake LP tokens (GM or GLV).
    Stake {
        /// LP token kind (GM or GLV).
        #[arg(long, value_enum)]
        kind: LpTokenKind,
        /// LP token mint address.
        lp_token_mint: Pubkey,
        /// Oracle buffer account address.
        #[arg(long)]
        oracle: Pubkey,
        /// Amount to stake (in raw token units).
        #[arg(long)]
        amount: u64,
        /// Optional position ID (if not provided, will generate randomly).
        #[arg(long)]
        position_id: Option<u64>,
    },
}

impl super::Command for Lp {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
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
            Command::DisableController { lp_token_mint } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                client
                    .disable_lp_token_controller(store, lp_token_mint)?
                    .into_bundle_with_options(options)?
            }
            Command::Stake {
                kind,
                lp_token_mint,
                oracle,
                amount,
                position_id,
            } => {
                use gmsol_sdk::ops::liquidity_provider::LiquidityProviderOps;

                // Convert amount to NonZeroU64
                let stake_amount = NonZeroU64::new(*amount)
                    .ok_or_else(|| eyre::eyre!("Stake amount must be greater than zero"))?;

                // Create stake builder
                let mut stake_builder =
                    client.stake_lp_token(store, *kind, lp_token_mint, oracle, stake_amount);

                // Set position ID if provided
                if let Some(pos_id) = position_id {
                    stake_builder = stake_builder.with_position_id(*pos_id);
                }

                // Build the bundle
                stake_builder.build_with_options(options).await?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
