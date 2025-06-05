use std::path::PathBuf;

use eyre::OptionExt;
use gmsol_sdk::{
    core::token_config::TokenMapAccess,
    ops::{OracleOps, StoreOps, TokenConfigOps},
    programs::anchor_lang::prelude::Pubkey,
    serde::{serde_token_map::SerdeTokenConfig, StringPubkey},
    solana_utils::solana_sdk::signature::{EncodableKey, Keypair},
};
use indexmap::IndexMap;
use rand::{rngs::StdRng, SeedableRng};

use crate::config::DisplayOptions;

use super::CommandClient;

/// Market management commands.
#[derive(Debug, clap::Args)]
pub struct Market {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Create an oracle buffer.
    CreateOracle {
        /// Path to the keypair of the account to be initialized as a oracle buffer.
        /// If not provided, a new keypair will be generated.
        oracle: Option<PathBuf>,
        /// Optional random seed to use for oracle buffer initialization.
        #[arg(long)]
        seed: Option<u64>,
        /// Pubkey of the authority for the oracle buffer.
        #[arg(long)]
        authority: Option<Pubkey>,
    },
    /// Display the token configs in the selected token map.
    Tokens {
        #[arg(long)]
        token_map: Option<Pubkey>,
        #[arg(group = "map-input")]
        token: Option<Pubkey>,
        #[arg(long, group = "map-input")]
        header: bool,
    },
    /// Create a new token map.
    CreateTokenMap {
        /// Path to the keypair of the account to be initialized as a token map.
        /// If not provided, a new keypair will be generated.
        token_map: Option<PathBuf>,
        /// Optional random seed to use for token map initialization.
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Set the selected token map as the authorized one.
    SetTokenMap { token_map: Pubkey },
}

impl super::Command for Market {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();
        let output = ctx.config().output();

        let bundle = match &self.command {
            Command::CreateOracle {
                oracle,
                seed,
                authority,
            } => {
                let oracle = match oracle {
                    Some(path) => {
                        Keypair::read_from_file(path).map_err(|err| eyre::eyre!("{err}"))?
                    }
                    None => {
                        let mut rng = if let Some(seed) = seed {
                            StdRng::seed_from_u64(*seed)
                        } else {
                            StdRng::from_entropy()
                        };
                        Keypair::generate(&mut rng)
                    }
                };
                let (rpc, oracle) = client
                    .initialize_oracle(store, &oracle, authority.as_ref())
                    .await?;
                println!("Oracle: {oracle}");
                let bundle = rpc.into_bundle_with_options(options)?;
                client.send_or_serialize(bundle).await?;
                return Ok(());
            }
            Command::Tokens {
                token_map,
                token,
                header,
            } => {
                let token_map_address = token_map_address(client, token_map.as_ref()).await?;
                let token_map = client.token_map(&token_map_address).await?;

                if let Some(token) = token {
                    let config = token_map.get(token).ok_or_eyre("token not found")?;
                    let serialized = SerdeTokenConfig::try_from(config)?;
                    println!(
                        "{}",
                        output.display_keyed_account(
                            token,
                            serialized,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("pubkey", "Pubkey"),
                                ("is_enabled", "Enabled"),
                                ("is_synthetic", "Synthetic"),
                                ("token_decimals", "Decimals"),
                                ("price_precision", "Price Precision"),
                                ("expected_provider", "Expected Provider"),
                                ("feeds.chainlink_data_streams.feed_id", "Chainlink Feed"),
                                (
                                    "feeds.chainlink_data_streams.timestamp_adjustment",
                                    "Chainlink TS Adj",
                                ),
                                ("feeds.pyth.feed_id", "Pyth Feed"),
                                ("feeds.pyth.timestamp_adjustment", "Pyth TS Adj",),
                                ("feeds.switchboard.feed_id", "Switchboard Feed"),
                                (
                                    "feeds.switchboard.timestamp_adjustment",
                                    "Switchboard TS Adj",
                                ),
                            ])
                        )?
                    );
                } else if *header {
                    let authorized_token_map_address =
                        client.authorized_token_map_address(store).await?;
                    let output = output.display_keyed_account(
                        &token_map_address,
                        serde_json::json!({
                            "store": StringPubkey(token_map.header().store),
                            "tokens": token_map.header().tokens.len(),
                            "is_authorized": authorized_token_map_address == Some(token_map_address),
                        }),
                        DisplayOptions::table_projection([
                            ("pubkey", "Address"),
                            ("tokens", "Tokens"),
                            ("is_authorized", "Authorized"),
                        ]),
                    )?;
                    println!("{output}");
                } else {
                    let mut map = token_map
                        .tokens()
                        .filter_map(|token| {
                            token_map
                                .get(&token)
                                .and_then(|config| SerdeTokenConfig::try_from(config).ok())
                                .map(|config| (token, config))
                        })
                        .collect::<IndexMap<_, _>>();
                    map.sort_by(|_, a, _, b| a.name.cmp(&b.name));
                    map.sort_by(|_, a, _, b| a.is_enabled.cmp(&b.is_enabled).reverse());
                    println!(
                        "{}",
                        output.display_keyed_accounts(
                            map,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("pubkey", "Pubkey"),
                                ("is_enabled", "Enabled"),
                                ("is_synthetic", "Synthetic"),
                                ("token_decimals", "Decimals"),
                                ("price_precision", "Price Precision"),
                                ("expected_provider", "Expected Provider"),
                            ])
                        )?
                    );
                }

                return Ok(());
            }
            Command::CreateTokenMap { token_map, seed } => {
                let token_map = match token_map {
                    Some(path) => {
                        Keypair::read_from_file(path).map_err(|err| eyre::eyre!("{err}"))?
                    }
                    None => {
                        let mut rng = if let Some(seed) = seed {
                            StdRng::seed_from_u64(*seed)
                        } else {
                            StdRng::from_entropy()
                        };
                        Keypair::generate(&mut rng)
                    }
                };
                let (rpc, token_map) = client.initialize_token_map(store, &token_map);
                println!("Token Map: {token_map}");
                let bundle = rpc.into_bundle_with_options(options)?;
                client.send_or_serialize(bundle).await?;
                return Ok(());
            }
            Command::SetTokenMap { token_map } => client
                .set_token_map(store, token_map)
                .into_bundle_with_options(options)?,
        };

        client.send_or_serialize(bundle).await?;

        Ok(())
    }
}

async fn token_map_address(
    client: &CommandClient,
    token_map: Option<&Pubkey>,
) -> eyre::Result<Pubkey> {
    let address = match token_map {
        Some(address) => *address,
        None => client
            .authorized_token_map_address(&client.store)
            .await?
            .ok_or_eyre("no authorized token map")?,
    };
    Ok(address)
}
