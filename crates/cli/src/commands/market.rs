use std::{num::NonZeroUsize, path::PathBuf};

use either::Either;
use eyre::OptionExt;
use gmsol_sdk::{
    core::token_config::TokenMapAccess,
    ops::{MarketOps, OracleOps, StoreOps, TokenConfigOps},
    programs::anchor_lang::prelude::Pubkey,
    serde::{serde_market::SerdeMarketConfig, serde_token_map::SerdeTokenConfig, StringPubkey},
    solana_utils::{
        bundle_builder::{BundleBuilder, BundleOptions},
        signer::LocalSignerRef,
        solana_sdk::{signature::Keypair, signer::Signer},
    },
    utils::market::MarketDecimals,
};
use indexmap::IndexMap;

use crate::{commands::utils::toml_from_file, config::DisplayOptions};

use super::{utils::KeypairArgs, CommandClient};

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
        #[command(flatten)]
        keypair: KeypairArgs,
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
        #[command(flatten)]
        keypair: KeypairArgs,
    },
    /// Set the selected token map as the authorized one.
    SetTokenMap { token_map: Pubkey },
    /// Create a `MarketConfigBuffer` account.
    CreateBuffer {
        #[command(flatten)]
        keypair: KeypairArgs,
        /// The buffer will expire after this duration.
        #[arg(long, default_value = "1d")]
        expire_after: humantime::Duration,
    },
    /// Close a `MarketConfigBuffer` account.
    CloseBuffer {
        /// Buffer account to close.
        buffer: Pubkey,
        /// Address to receive the lamports.
        #[arg(long)]
        receiver: Option<Pubkey>,
    },
    /// Set the authority of the `MarketConfigBuffer` account.
    SetBufferAuthority {
        /// Buffer account of which to set the authority.
        buffer: Pubkey,
        /// New authority.
        #[arg(long)]
        new_authority: Pubkey,
    },
    /// Push to `MarketConfigBuffer` account with configs read from file.
    PushToBuffer {
        /// Path to the config file to read from.
        #[arg(requires = "buffer-input")]
        path: PathBuf,
        /// Buffer account to be pushed to.
        #[arg(long, group = "buffer-input")]
        buffer: Option<Pubkey>,
        /// Whether to create a new buffer account.
        #[arg(long, group = "buffer-input")]
        init: bool,
        /// The expected market token to use for this buffer.
        #[arg(long)]
        market_token: Pubkey,
        /// The number of keys to push in single instruction.
        #[arg(long, default_value = "16")]
        batch: NonZeroUsize,
        /// The buffer will expire after this duration.
        /// Only effective when used with `--init`.
        #[arg(long, default_value = "1d")]
        expire_after: humantime::Duration,
    },
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
            Command::CreateOracle { keypair, authority } => {
                let oracle = keypair.to_keypair()?;
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
            Command::CreateTokenMap { keypair } => {
                let token_map = keypair.to_keypair()?;
                let (rpc, token_map) = client.initialize_token_map(store, &token_map);
                println!("Token Map: {token_map}");
                let bundle = rpc.into_bundle_with_options(options)?;
                client.send_or_serialize(bundle).await?;
                return Ok(());
            }
            Command::SetTokenMap { token_map } => client
                .set_token_map(store, token_map)
                .into_bundle_with_options(options)?,
            Command::CreateBuffer {
                keypair,
                expire_after,
            } => {
                let buffer_keypair = keypair.to_keypair()?;
                let rpc = client.initialize_market_config_buffer(
                    store,
                    &buffer_keypair,
                    expire_after.as_secs().try_into()?,
                );

                client
                    .send_or_serialize(rpc.into_bundle_with_options(options)?)
                    .await?;
                return Ok(());
            }
            Command::CloseBuffer { buffer, receiver } => client
                .close_marekt_config_buffer(buffer, receiver.as_ref())
                .into_bundle_with_options(options)?,
            Command::SetBufferAuthority {
                buffer,
                new_authority,
            } => client
                .set_market_config_buffer_authority(buffer, new_authority)
                .into_bundle_with_options(options)?,
            Command::PushToBuffer {
                path,
                buffer,
                init,
                market_token,
                batch,
                expire_after,
            } => {
                let config: Either<MarketConfigs, SerdeMarketConfig> = toml_from_file(path)?;
                let config = match config {
                    Either::Left(configs) => {
                        let config = configs
                            .configs
                            .get(market_token)
                            .ok_or_eyre(format!("the config for `{market_token}` not found"))?;
                        config.config.clone()
                    }
                    Either::Right(config) => config,
                };
                assert!(buffer.is_none() == *init, "must hold");
                let keypair = Keypair::new();
                let buffer = match buffer {
                    Some(buffer) => Either::Left(buffer),
                    None => Either::Right(&keypair),
                };
                let bundle = push_to_market_config_buffer(
                    client,
                    buffer,
                    market_token,
                    &config,
                    expire_after,
                    *batch,
                    options,
                )
                .await?;
                client.send_or_serialize(bundle).await?;
                return Ok(());
            }
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

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct MarketConfig {
    #[serde(default)]
    enable: Option<bool>,
    #[serde(default)]
    buffer: Option<StringPubkey>,
    #[serde(flatten)]
    config: SerdeMarketConfig,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MarketConfigs {
    #[serde(flatten)]
    configs: IndexMap<StringPubkey, MarketConfig>,
}

async fn push_to_market_config_buffer<'a>(
    client: &'a CommandClient,
    buffer: Either<&Pubkey, &'a Keypair>,
    market_token: &Pubkey,
    config: &SerdeMarketConfig,
    expire_after: &humantime::Duration,
    batch: NonZeroUsize,
    options: BundleOptions,
) -> eyre::Result<BundleBuilder<'a, LocalSignerRef>> {
    let store = &client.store;
    let market = client.market_by_token(store, market_token).await?;
    let token_map = client.authorized_token_map(store).await?;
    let decimals = MarketDecimals::new(&market.meta.into(), &token_map)?;

    let mut bundle = client.bundle_with_options(options);

    let buffer = match buffer {
        Either::Left(pubkey) => *pubkey,
        Either::Right(keypair) => {
            bundle.push(client.initialize_market_config_buffer(
                store,
                keypair,
                expire_after.as_secs().try_into().unwrap_or(u32::MAX),
            ))?;
            keypair.pubkey()
        }
    };

    println!("Buffer: {buffer}");

    let configs = config
        .0
        .iter()
        .map(|(k, v)| Ok((k, v.to_u128(decimals.market_config_decimals(*k)?)?)))
        .collect::<eyre::Result<Vec<_>>>()?;
    for batch in configs.chunks(batch.get()) {
        bundle.push(client.push_to_market_config_buffer(
            &buffer,
            batch.iter().map(|(key, value)| (key, *value)),
        ))?;
    }

    Ok(bundle)
}
