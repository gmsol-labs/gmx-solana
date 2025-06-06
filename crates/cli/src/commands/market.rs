use std::{num::NonZeroUsize, path::PathBuf};

use either::Either;
use eyre::OptionExt;
use gmsol_sdk::{
    core::{
        oracle::PriceProviderKind,
        token_config::{
            TokenMapAccess, UpdateTokenConfigParams, DEFAULT_HEARTBEAT_DURATION, DEFAULT_PRECISION,
            DEFAULT_TIMESTAMP_ADJUSTMENT,
        },
    },
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

use super::{
    utils::{KeypairArgs, ToggleValue},
    CommandClient,
};

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
    /// Insert token configs from file.
    InsertTokenConfigs {
        #[arg(long)]
        token_map: Option<Pubkey>,
        #[arg(long)]
        set_token_map: bool,
        path: PathBuf,
    },
    /// Toggle the token config for the given token.
    ToggleTokenConfig {
        #[arg(long)]
        token_map: Option<Pubkey>,
        token: Pubkey,
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Set expected provider of token.
    SetExpectedProvider {
        #[arg(long)]
        token_map: Option<Pubkey>,
        token: Pubkey,
        provider: PriceProviderKind,
    },
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
            Command::InsertTokenConfigs {
                path,
                token_map,
                set_token_map,
            } => {
                let configs: IndexMap<String, TokenConfig> = toml_from_file(path)?;
                let token_map = token_map_address(client, token_map.as_ref()).await?;
                insert_token_configs(client, &token_map, *set_token_map, &configs, options)?
            }
            Command::ToggleTokenConfig {
                token,
                token_map,
                toggle,
            } => {
                let token_map_address = token_map_address(client, token_map.as_ref()).await?;
                client
                    .toggle_token_config(store, &token_map_address, token, toggle.is_enable())
                    .into_bundle_with_options(options)?
            }
            Command::SetExpectedProvider {
                token_map,
                token,
                provider,
            } => {
                let token_map_address = token_map_address(client, token_map.as_ref()).await?;
                client
                    .set_expected_provider(store, &token_map_address, token, *provider)
                    .into_bundle_with_options(options)?
            }
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct TokenConfig {
    address: StringPubkey,
    #[serde(default)]
    synthetic: Option<u8>,
    enable: bool,
    expected_provider: PriceProviderKind,
    feeds: Feeds,
    #[serde(default = "default_precision")]
    precision: u8,
    #[serde(default = "default_heartbeat_duration")]
    heartbeat_duration: u32,
    #[serde(default)]
    update: bool,
}

fn default_heartbeat_duration() -> u32 {
    DEFAULT_HEARTBEAT_DURATION
}

fn default_precision() -> u8 {
    DEFAULT_PRECISION
}

impl<'a> TryFrom<&'a TokenConfig> for UpdateTokenConfigParams {
    type Error = eyre::Error;

    fn try_from(config: &'a TokenConfig) -> Result<Self, Self::Error> {
        let mut builder = Self::default()
            .with_expected_provider(config.expected_provider)
            .with_heartbeat_duration(config.heartbeat_duration)
            .with_precision(config.precision);
        if let Some(feed_id) = config.feeds.switchboard_feed_id()? {
            builder = builder.update_price_feed(
                &PriceProviderKind::Switchboard,
                feed_id,
                Some(config.feeds.switchboard_feed_timestamp_adjustment),
            )?;
        }
        if let Some(pyth_feed_id) = config.feeds.pyth_feed_id()? {
            builder = builder.update_price_feed(
                &PriceProviderKind::Pyth,
                pyth_feed_id,
                Some(config.feeds.pyth_feed_timestamp_adjustment),
            )?;
        }
        if let Some(feed_id) = config.feeds.chainlink_data_streams_feed_id()? {
            builder = builder.update_price_feed(
                &PriceProviderKind::ChainlinkDataStreams,
                feed_id,
                Some(
                    config
                        .feeds
                        .chainlink_data_streams_feed_timestamp_adjustment,
                ),
            )?;
        }
        Ok(builder)
    }
}

#[derive(Debug, clap::Args, serde::Serialize, serde::Deserialize)]
#[group(required = true, multiple = true)]
struct Feeds {
    /// Switchboard feed id.
    #[arg(long)]
    switchboard_feed_id: Option<String>,
    /// Switchboard feed timestamp adjustment.
    #[arg(long, default_value_t = DEFAULT_TIMESTAMP_ADJUSTMENT)]
    #[serde(default = "default_timestamp_adjustment")]
    switchboard_feed_timestamp_adjustment: u32,
    /// Pyth feed id.
    #[arg(long)]
    pyth_feed_id: Option<String>,
    /// Pyth feed timestamp adjustment.
    #[arg(long, default_value_t = DEFAULT_TIMESTAMP_ADJUSTMENT)]
    #[serde(default = "default_timestamp_adjustment")]
    pyth_feed_timestamp_adjustment: u32,
    /// Chainlink Data Streams feed id.
    #[arg(long)]
    chainlink_data_streams_feed_id: Option<String>,
    #[arg(long, default_value_t = DEFAULT_TIMESTAMP_ADJUSTMENT)]
    #[serde(default = "default_timestamp_adjustment")]
    chainlink_data_streams_feed_timestamp_adjustment: u32,
}

fn default_timestamp_adjustment() -> u32 {
    DEFAULT_TIMESTAMP_ADJUSTMENT
}

impl Feeds {
    fn pyth_feed_id(&self) -> eyre::Result<Option<Pubkey>> {
        use gmsol_sdk::client::pyth::pull_oracle::hermes::Identifier;

        let Some(pyth_feed_id) = self.pyth_feed_id.as_ref() else {
            return Ok(None);
        };
        let feed_id = pyth_feed_id.strip_prefix("0x").unwrap_or(pyth_feed_id);
        let feed_id = Identifier::from_hex(feed_id)?;
        let feed_id_as_key = Pubkey::new_from_array(feed_id.to_bytes());
        Ok(Some(feed_id_as_key))
    }

    fn chainlink_data_streams_feed_id(&self) -> eyre::Result<Option<Pubkey>> {
        use gmsol_sdk::client::chainlink::pull_oracle::parse_feed_id;

        let Some(feed_id) = self.chainlink_data_streams_feed_id.as_ref() else {
            return Ok(None);
        };
        let feed_id = parse_feed_id(feed_id)?;
        let feed_id_as_key = Pubkey::new_from_array(feed_id);
        Ok(Some(feed_id_as_key))
    }

    fn switchboard_feed_id(&self) -> eyre::Result<Option<Pubkey>> {
        let Some(feed_id) = self.switchboard_feed_id.as_ref() else {
            return Ok(None);
        };
        let feed_id_as_key = feed_id.parse()?;
        Ok(Some(feed_id_as_key))
    }
}

fn insert_token_configs<'a>(
    client: &'a CommandClient,
    token_map: &Pubkey,
    set_token_map: bool,
    configs: &IndexMap<String, TokenConfig>,
    options: BundleOptions,
) -> eyre::Result<BundleBuilder<'a, LocalSignerRef>> {
    let store = &client.store;
    let mut bundle = client.bundle_with_options(options);

    if set_token_map {
        bundle.push(client.set_token_map(store, token_map))?;
    }

    for (name, config) in configs {
        let token = &config.address;
        if let Some(decimals) = config.synthetic {
            bundle.push(client.insert_synthetic_token_config(
                store,
                token_map,
                name,
                token,
                decimals,
                config.try_into()?,
                config.enable,
                !config.update,
            ))?;
        } else {
            bundle.push(client.insert_token_config(
                store,
                token_map,
                name,
                token,
                config.try_into()?,
                config.enable,
                !config.update,
            ))?;
        }
    }

    Ok(bundle)
}
