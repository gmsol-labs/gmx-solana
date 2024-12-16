use std::{
    collections::HashSet,
    io::Read,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anchor_client::solana_sdk::{
    pubkey::Pubkey,
    signature::Keypair,
    signer::{EncodableKey, Signer},
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol::{
    constants,
    exchange::ExchangeOps,
    store::{
        gt::GtOps,
        market::{MarketOps, VaultOps},
        oracle::OracleOps,
        store_ops::StoreOps,
        token_config::TokenConfigOps,
    },
    types::{
        market::config::{MarketConfigBuffer, MarketConfigFlag},
        FactorKey,
    },
    utils::TransactionBuilder,
};
use gmsol_store::states::{
    Factor, MarketConfigKey, PriceProviderKind, TokenConfigBuilder, DEFAULT_HEARTBEAT_DURATION,
    DEFAULT_PRECISION,
};
use indexmap::IndexMap;
use rand::{rngs::StdRng, SeedableRng};
use serde::de::DeserializeOwned;

use crate::{ser::MarketConfigMap, GMSOLClient};

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(long)]
    token_map: Option<Pubkey>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create an `Oracle` account.
    CreateOracle {},
    /// Create a `TokenMap` account.
    CreateTokenMap,
    /// Create a `MarketConfigBuffer` account.
    CreateBuffer {
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
        /// Select config with this market token.
        /// Pass this option to allow reading from multi-markets config format.
        #[arg(long, short)]
        market_token: Option<Pubkey>,
        /// Skip prefligh test.
        #[arg(long)]
        skip_preflight: bool,
        /// Max transaction size.
        #[arg(long)]
        max_transaction_size: Option<usize>,
        /// The number of keys to push in single instruction.
        #[arg(long, default_value = "16")]
        batch: NonZeroUsize,
        /// The buffer will expire after this duration.
        /// Only effective when used with `--init`.
        #[arg(long, default_value = "1d")]
        expire_after: humantime::Duration,
    },
    /// Set the authority of the `MarketConfigBuffer` account.
    SetBufferAuthority {
        /// Buffer account of which to set the authority.
        buffer: Pubkey,
        /// New authority.
        #[arg(long)]
        new_authority: Pubkey,
    },
    /// Set token map.
    SetTokenMap { token_map: Pubkey },
    /// Read and insert token configs from file.
    InsertTokenConfigs {
        path: PathBuf,
        #[arg(long)]
        token_map: Option<Pubkey>,
        #[arg(long)]
        skip_preflight: bool,
        #[arg(long)]
        set_token_map: bool,
        #[arg(long)]
        max_transaction_size: Option<usize>,
    },
    /// Insert or update the config of token.
    InsertTokenConfig {
        token: Pubkey,
        #[arg(long)]
        name: String,
        #[command(flatten)]
        feeds: Feeds,
        #[arg(long)]
        expected_provider: PriceProviderKind,
        #[arg(long, default_value_t = DEFAULT_HEARTBEAT_DURATION)]
        heartbeat_duration: u32,
        #[arg(long, default_value_t = DEFAULT_PRECISION)]
        precision: u8,
        /// Provide to create a synthetic token with the given decimals.
        #[arg(long, value_name = "DECIMALS")]
        synthetic: Option<u8>,
        #[arg(long)]
        update: bool,
    },
    /// Toggle token config of token.
    ToggleTokenConfig {
        token: Pubkey,
        #[command(flatten)]
        toggle: Toggle,
    },
    /// Set expected provider of token.
    SetExpectedProvider {
        token: Pubkey,
        provider: PriceProviderKind,
    },
    /// Create Market Vault.
    CreateVault { token: Pubkey },
    /// Create Market.
    CreateMarket {
        #[arg(long)]
        name: String,
        #[arg(long)]
        index_token: Pubkey,
        #[arg(long)]
        long_token: Pubkey,
        #[arg(long)]
        short_token: Pubkey,
        #[arg(long)]
        enable: bool,
    },
    /// Create Markets from file.
    CreateMarkets {
        path: PathBuf,
        #[arg(long)]
        skip_preflight: bool,
        #[arg(long)]
        enable: bool,
        #[arg(long)]
        max_transaction_size: Option<usize>,
    },
    /// Update Market Config.
    UpdateConfig {
        /// The market token of the market to update.
        #[arg(requires = "config")]
        market_token: Pubkey,
        /// The config key to udpate.
        #[arg(long, short, group = "config", requires = "value")]
        key: Option<MarketConfigKey>,
        /// The value that the config to update to.
        #[arg(long, short)]
        value: Option<Factor>,
        /// Update market config with this buffer.
        #[arg(long, group = "config")]
        buffer: Option<Pubkey>,
        /// Recevier for the buffer's lamports.
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Whether to keep the used market config buffer account.
        #[arg(long)]
        keep_buffer: bool,
    },
    /// Update Market Config Flag.
    UpdateConfigFlag {
        /// The market token of the market to update.
        market_token: Pubkey,
        /// The config key to udpate.
        #[arg(long, short)]
        key: MarketConfigFlag,
        /// The boolean value that the flag to update to.
        #[arg(long, short)]
        value: bool,
    },
    /// Update Market Configs from file.
    UpdateConfigs {
        path: PathBuf,
        #[arg(long)]
        skip_preflight: bool,
        #[arg(long)]
        max_transaction_size: Option<usize>,
        /// Recevier for the buffer's lamports.
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Whether to keep the used market config buffer accounts.
        #[arg(long)]
        keep_buffers: bool,
    },
    /// Toggle market.
    ToggleMarket {
        market_token: Pubkey,
        #[command(flatten)]
        toggle: Toggle,
    },
    /// Fund Market.
    FundMarket {
        /// The address of the market token of the Market to fund
        market_token: Pubkey,
        /// The funding side.
        #[arg(long)]
        side: Side,
        /// The funding amount.
        #[arg(long, short)]
        amount: u64,
    },
    /// Toggle GT minting.
    ToggleGtMinting {
        market_token: Pubkey,
        #[command(flatten)]
        toggle: Toggle,
    },
    /// Initialize GT.
    InitGt {
        #[arg(long, short, default_value_t = 7)]
        decimals: u8,
        #[arg(long, short = 'c', default_value_t = 100 * constants::MARKET_USD_UNIT / 10u128.pow(7))]
        initial_minting_cost: u128,
        #[arg(long, default_value_t = 101 * constants::MARKET_USD_UNIT / 100)]
        grow_factor: u128,
        #[arg(long, default_value_t = 10 * 10u64.pow(7))]
        grow_step: u64,
        ranks: Vec<u64>,
    },
    /// Set order fee discount factors.
    SetOrderFeeDiscountFactors { factors: Vec<u128> },
    /// Set referral reward factors.
    SetReferralRewardFactors { factors: Vec<u128> },
    /// Set referred discount.
    SetReferredDiscountFactor { factor: u128 },
    /// Initialize Oracle.
    InitOracle {
        wallet: Option<PathBuf>,
        #[arg(long, short)]
        seed: Option<u64>,
    },
}

#[serde_with::serde_as]
#[derive(Debug, clap::Args, serde::Serialize, serde::Deserialize)]
#[group(required = true, multiple = true)]
struct Feeds {
    /// Pyth feed id.
    #[arg(long)]
    pyth_feed_id: Option<String>,
    /// Chainlink feed.
    #[arg(long)]
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    chainlink_feed: Option<Pubkey>,
}

impl Feeds {
    fn pyth_feed_id(&self) -> gmsol::Result<Option<Pubkey>> {
        let Some(pyth_feed_id) = self.pyth_feed_id.as_ref() else {
            return Ok(None);
        };
        let feed_id = pyth_feed_id.strip_prefix("0x").unwrap_or(pyth_feed_id);
        let feed_id = pyth_sdk::Identifier::from_hex(feed_id).map_err(gmsol::Error::unknown)?;
        let feed_id_as_key = Pubkey::new_from_array(feed_id.to_bytes());
        Ok(Some(feed_id_as_key))
    }
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct Toggle {
    #[arg(long)]
    enable: bool,
    #[arg(long)]
    disable: bool,
}

impl Toggle {
    fn is_enable(&self) -> bool {
        debug_assert!(self.enable != self.disable);
        self.enable
    }
}

#[derive(clap::ValueEnum, Clone)]
enum Side {
    Long,
    Short,
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::CreateOracle {} => {
                let oracle_keypair = Keypair::new();
                let (request, oracle) = client.initialize_oracle(store, &oracle_keypair).await?;
                crate::utils::send_or_serialize(
                    request.into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!("created oracle {oracle} at tx {signature}");
                        println!("{oracle}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::CreateTokenMap => {
                if serialize_only {
                    return Err(gmsol::Error::invalid_argument(
                        "serialize-only mode is not supported for this command",
                    ));
                }
                let token_map = Keypair::new();
                let (rpc, map) = client.initialize_token_map(store, &token_map);
                crate::utils::send_or_serialize(
                    rpc.into_anchor_request_without_compute_budget(),
                    false,
                    |signature| {
                        tracing::info!("created token config map {map} at tx {signature}");
                        println!("{map}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::SetTokenMap { token_map } => {
                crate::utils::send_or_serialize(
                    client
                        .set_token_map(store, token_map)
                        .into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!("set new token map at {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::InsertTokenConfigs {
                path,
                token_map,
                skip_preflight,
                set_token_map,
                max_transaction_size,
            } => {
                let configs: IndexMap<String, TokenConfig> = toml_from_file(path)?;
                let token_map = match token_map {
                    Some(token_map) => {
                        if *set_token_map {
                            let authorized_token_map =
                                client.authorized_token_map_address(store).await?;
                            if authorized_token_map == Some(*token_map) {
                                return Err(gmsol::Error::invalid_argument("the token map has been authorized, please remove `--set-token-map` and try again"));
                            }
                        }
                        *token_map
                    }
                    None => client
                        .authorized_token_map_address(store)
                        .await?
                        .ok_or(gmsol::Error::NotFound)?,
                };

                insert_token_configs(
                    client,
                    store,
                    &token_map,
                    serialize_only,
                    *skip_preflight,
                    *set_token_map,
                    *max_transaction_size,
                    &configs,
                )
                .await?;
            }
            Command::InsertTokenConfig {
                name,
                token,
                feeds,
                expected_provider,
                heartbeat_duration,
                precision,
                synthetic: fake_decimals,
                update,
            } => {
                let mut builder = TokenConfigBuilder::default()
                    .with_heartbeat_duration(*heartbeat_duration)
                    .with_precision(*precision)
                    .with_expected_provider(*expected_provider);
                if let Some(feed_id) = feeds.pyth_feed_id()? {
                    builder = builder
                        .update_price_feed(&PriceProviderKind::Pyth, feed_id)
                        .map_err(anchor_client::ClientError::from)?;
                }
                if let Some(feed) = feeds.chainlink_feed {
                    builder = builder
                        .update_price_feed(&PriceProviderKind::Chainlink, feed)
                        .map_err(anchor_client::ClientError::from)?;
                }
                let token_map = self.token_map(client, store).await?;
                let req = if let Some(decimals) = fake_decimals {
                    client.insert_synthetic_token_config(
                        store, &token_map, name, token, *decimals, builder, true, !*update,
                    )
                } else {
                    client.insert_token_config(
                        store, &token_map, name, token, builder, true, !*update,
                    )
                };
                crate::utils::send_or_serialize(
                    req.into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::ToggleTokenConfig { token, toggle } => {
                let token_map = self.token_map(client, store).await?;
                crate::utils::send_or_serialize(
                    client
                        .toggle_token_config(store, &token_map, token, toggle.is_enable())
                        .into_anchor_request(),
                    serialize_only,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::SetExpectedProvider { token, provider } => {
                let token_map = self.token_map(client, store).await?;
                crate::utils::send_or_serialize(
                    client
                        .set_expected_provider(store, &token_map, token, *provider)
                        .into_anchor_request(),
                    serialize_only,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::CreateVault { token } => {
                let (rpc, vault) = client.initialize_market_vault(store, token);
                crate::utils::send_or_serialize(
                    rpc.into_anchor_request(),
                    serialize_only,
                    |signature| {
                        tracing::info!("created a new vault {vault} at tx {signature}");
                        println!("{vault}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::CreateMarket {
                name,
                index_token,
                long_token,
                short_token,
                enable,
            } => {
                let (request, market_token) = client
                    .create_market(
                        store,
                        name,
                        index_token,
                        long_token,
                        short_token,
                        *enable,
                        None,
                    )
                    .await?;
                crate::utils::send_or_serialize(request.into_anchor_request_without_compute_budget(), serialize_only, |signature| {
                    tracing::info!(
                        "created a new market with {market_token} as its token address at tx {signature}"
                    );
                    println!("{market_token}");
                    Ok(())
                }).await?;
            }
            Command::CreateMarkets {
                path,
                skip_preflight,
                enable,
                max_transaction_size,
            } => {
                let markets: IndexMap<String, Market> = toml_from_file(path)?;
                create_markets(
                    client,
                    store,
                    serialize_only,
                    *skip_preflight,
                    *enable,
                    *max_transaction_size,
                    &markets,
                )
                .await?;
            }
            Command::UpdateConfig {
                market_token,
                key,
                value,
                buffer,
                receiver,
                keep_buffer,
            } => {
                let config = MarketConfig {
                    enable: None,
                    buffer: *buffer,
                    config: MarketConfigMap(
                        key.map(|key| (key, value.expect("missing value").into()))
                            .into_iter()
                            .collect(),
                    ),
                };
                let configs = MarketConfigs {
                    configs: [(*market_token, config)].into(),
                };
                configs
                    .update(
                        client,
                        store,
                        serialize_only,
                        false,
                        None,
                        receiver.as_ref(),
                        !*keep_buffer,
                    )
                    .await?;
            }
            Command::UpdateConfigFlag {
                market_token,
                key,
                value,
            } => {
                crate::utils::send_or_serialize_rpc(
                    client.update_market_config_flag_by_key(store, market_token, *key, *value)?,
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!(
                            "market config flag is updated to {value} at tx {signature}"
                        );
                        Ok(())
                    },
                )
                .await?;
            }
            Command::UpdateConfigs {
                path,
                skip_preflight,
                max_transaction_size,
                receiver,
                keep_buffers,
            } => {
                let configs: MarketConfigs = toml_from_file(path)?;
                configs
                    .update(
                        client,
                        store,
                        serialize_only,
                        *skip_preflight,
                        *max_transaction_size,
                        receiver.as_ref(),
                        !*keep_buffers,
                    )
                    .await?;
            }
            Command::ToggleMarket {
                market_token,
                toggle,
            } => {
                crate::utils::send_or_serialize(
                    client
                        .toggle_market(store, market_token, toggle.is_enable())
                        .into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!(
                            "market set to be {} at tx {signature}",
                            if toggle.is_enable() {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                        Ok(())
                    },
                )
                .await?;
            }
            Command::CreateBuffer { expire_after } => {
                if serialize_only {
                    return Err(gmsol::Error::invalid_argument(
                        "serialize-only mode is not supported for this command",
                    ));
                }
                let buffer_keypair = Keypair::new();
                let rpc = client.initialize_market_config_buffer(
                    store,
                    &buffer_keypair,
                    expire_after.as_secs().try_into().unwrap_or(u32::MAX),
                );
                crate::utils::send_or_serialize(
                    rpc.into_anchor_request_without_compute_budget(),
                    false,
                    |signature| {
                        let pubkey = buffer_keypair.pubkey();
                        tracing::info!("created market config buffer `{pubkey}` at tx {signature}");
                        println!("{pubkey}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::CloseBuffer { buffer, receiver } => {
                crate::utils::send_or_serialize(
                    client
                        .close_marekt_config_buffer(buffer, receiver.as_ref())
                        .into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!("market config buffer `{buffer}` closed at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::PushToBuffer {
                path,
                buffer,
                init,
                market_token,
                skip_preflight,
                max_transaction_size,
                batch,
                expire_after,
            } => {
                let config = if let Some(market_token) = market_token {
                    let configs: MarketConfigs = toml_from_file(path)?;
                    let Some(config) = configs.configs.get(market_token) else {
                        return Err(gmsol::Error::invalid_argument(format!(
                            "the config for `{market_token}` not found"
                        )));
                    };
                    config.clone().config
                } else {
                    let config: MarketConfigMap = toml_from_file(path)?;
                    config
                };
                assert!(buffer.is_none() == *init, "must hold");
                config
                    .update(
                        client,
                        store,
                        buffer.as_ref(),
                        expire_after,
                        serialize_only,
                        *skip_preflight,
                        *max_transaction_size,
                        *batch,
                    )
                    .await?;
            }
            Command::SetBufferAuthority {
                buffer,
                new_authority,
            } => {
                crate::utils::send_or_serialize(
                    client
                        .set_market_config_buffer_authority(buffer, new_authority)
                        .into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!("set the authority of buffer `{buffer}` to `{new_authority}` at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::FundMarket {
                market_token,
                side,
                amount,
            } => {
                let market = client
                    .market(&client.find_market_address(store, market_token))
                    .await?;
                let token = match side {
                    Side::Long => market.meta().long_token_mint,
                    Side::Short => market.meta().short_token_mint,
                };
                let source_account = get_associated_token_address(&client.payer(), &token);
                crate::utils::send_or_serialize(
                    client
                        .fund_market(store, market_token, &source_account, *amount, Some(&token))
                        .await?
                        .into_anchor_request_without_compute_budget(),
                    serialize_only,
                    |signature| {
                        tracing::info!("funded at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::ToggleGtMinting {
                market_token,
                toggle,
            } => {
                crate::utils::send_or_serialize_rpc(
                    client.toggle_gt_minting(store, market_token, toggle.is_enable()),
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!(
                            %market_token,
                            "GT minting set to be {} at tx {signature}",
                            if toggle.is_enable() {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                        Ok(())
                    },
                )
                .await?;
            }
            Command::InitGt {
                decimals,
                initial_minting_cost,
                grow_factor,
                grow_step,
                ranks,
            } => {
                if ranks.is_empty() {
                    return Err(gmsol::Error::invalid_argument("ranks must be provided"));
                }
                let mut ranks = ranks.clone();
                ranks.sort_unstable();
                crate::utils::send_or_serialize_rpc(
                    client.initialize_gt(
                        store,
                        *decimals,
                        *initial_minting_cost,
                        *grow_factor,
                        *grow_step,
                        ranks.clone(),
                    ),
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!("initialized GT at tx {signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::SetOrderFeeDiscountFactors { factors } => {
                if factors.is_empty() {
                    return Err(gmsol::Error::invalid_argument("factors must be provided"));
                }
                crate::utils::send_or_serialize_rpc(
                    client.gt_set_order_fee_discount_factors(store, factors.clone()),
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!("set order fee discount factors at tx {signature}");
                        Ok(())
                    },
                )
                .await?
            }
            Command::SetReferralRewardFactors { factors } => {
                if factors.is_empty() {
                    return Err(gmsol::Error::invalid_argument("factors must be provided"));
                }
                crate::utils::send_or_serialize_rpc(
                    client.gt_set_referral_reward_factors(store, factors.clone()),
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!("set referral reward factors at tx {signature}");
                        Ok(())
                    },
                )
                .await?
            }
            Command::SetReferredDiscountFactor { factor } => {
                crate::utils::send_or_serialize_rpc(
                    client.insert_factor(
                        store,
                        FactorKey::OrderFeeDiscountForReferredUser,
                        *factor,
                    ),
                    serialize_only,
                    false,
                    |signature| {
                        tracing::info!("set referred discount factor at tx {signature}");
                        Ok(())
                    },
                )
                .await?
            }
            Command::InitOracle { wallet, seed } => {
                let oracle = match wallet {
                    Some(path) => {
                        Keypair::read_from_file(path).map_err(gmsol::Error::invalid_argument)?
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
                let (rpc, oracle) = client.initialize_oracle(store, &oracle).await?;
                crate::utils::send_or_serialize_rpc(rpc, serialize_only, false, |signature| {
                    tracing::info!("initialized an oracle buffer account at tx {signature}");
                    println!("{oracle}");
                    Ok(())
                })
                .await?
            }
        }
        Ok(())
    }

    async fn token_map(&self, client: &GMSOLClient, store: &Pubkey) -> gmsol::Result<Pubkey> {
        if let Some(token_map) = self.token_map {
            Ok(token_map)
        } else {
            Ok(client
                .authorized_token_map_address(store)
                .await?
                .ok_or(gmsol::Error::NotFound)?)
        }
    }
}

/// Token Config.
#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TokenConfig {
    #[serde_as(as = "serde_with::DisplayFromStr")]
    address: Pubkey,
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

impl<'a> TryFrom<&'a TokenConfig> for TokenConfigBuilder {
    type Error = gmsol::Error;

    fn try_from(config: &'a TokenConfig) -> Result<Self, Self::Error> {
        let mut builder = Self::default()
            .with_expected_provider(config.expected_provider)
            .with_heartbeat_duration(config.heartbeat_duration)
            .with_precision(config.precision);
        if let Some(pyth_feed_id) = config.feeds.pyth_feed_id()? {
            builder = builder.update_price_feed(&PriceProviderKind::Pyth, pyth_feed_id)?;
        }
        if let Some(chainlink_feed) = config.feeds.chainlink_feed {
            builder = builder.update_price_feed(&PriceProviderKind::Chainlink, chainlink_feed)?;
        }
        Ok(builder)
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_token_configs(
    client: &GMSOLClient,
    store: &Pubkey,
    token_map: &Pubkey,
    serialize_only: bool,
    skip_preflight: bool,
    set_token_map: bool,
    max_transaction_size: Option<usize>,
    configs: &IndexMap<String, TokenConfig>,
) -> gmsol::Result<()> {
    let mut builder = TransactionBuilder::new_with_options(
        client.store_program().solana_rpc(),
        false,
        max_transaction_size,
    );

    if set_token_map {
        builder.try_push(client.set_token_map(store, token_map))?;
    }

    for (name, config) in configs {
        let token = &config.address;
        if let Some(decimals) = config.synthetic {
            builder.try_push(client.insert_synthetic_token_config(
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
            builder.try_push(client.insert_token_config(
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

    crate::utils::send_or_serialize_transactions(
        builder,
        serialize_only,
        skip_preflight,
        |signatures, error| {
            tracing::info!("{signatures:#?}");
            match error {
                None => Ok(()),
                Some(err) => Err(err),
            }
        },
    )
    .await?;

    Ok(())
}

/// Market
#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Market {
    #[serde_as(as = "serde_with::DisplayFromStr")]
    index_token: Pubkey,
    #[serde_as(as = "serde_with::DisplayFromStr")]
    long_token: Pubkey,
    #[serde_as(as = "serde_with::DisplayFromStr")]
    short_token: Pubkey,
}

async fn create_markets(
    client: &GMSOLClient,
    store: &Pubkey,
    serialize_only: bool,
    skip_preflight: bool,
    enable: bool,
    max_transaction_size: Option<usize>,
    markets: &IndexMap<String, Market>,
) -> gmsol::Result<()> {
    let mut builder = TransactionBuilder::new_with_options(
        client.store_program().solana_rpc(),
        false,
        max_transaction_size,
    );
    let token_map = client
        .authorized_token_map_address(store)
        .await?
        .ok_or(gmsol::Error::NotFound)?;
    for (name, market) in markets {
        let (rpc, token) = client
            .create_market(
                store,
                name,
                &market.index_token,
                &market.long_token,
                &market.short_token,
                enable,
                Some(&token_map),
            )
            .await?;
        tracing::info!("Adding instruction to create market `{name}` with token={token}");
        builder.try_push(rpc)?;
    }

    crate::utils::send_or_serialize_transactions(
        builder,
        serialize_only,
        skip_preflight,
        |signatures, error| {
            println!("{signatures:#?}");
            match error {
                None => Ok(()),
                Some(err) => Err(err),
            }
        },
    )
    .await?;
    Ok(())
}

/// Market Configs.
#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketConfigs {
    #[serde_as(as = "IndexMap<serde_with::DisplayFromStr, _>")]
    #[serde(flatten)]
    configs: IndexMap<Pubkey, MarketConfig>,
}

impl MarketConfigMap {
    #[allow(clippy::too_many_arguments)]
    async fn update(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        buffer: Option<&Pubkey>,
        expire_after: &humantime::Duration,
        serialize_only: bool,
        skip_preflight: bool,
        max_transaction_size: Option<usize>,
        batch: NonZeroUsize,
    ) -> gmsol::Result<()> {
        let mut builder = TransactionBuilder::new_with_options(
            client.store_program().solana_rpc(),
            false,
            max_transaction_size,
        );

        let buffer_keypair = Keypair::new();
        let buffer = if let Some(buffer) = buffer {
            *buffer
        } else {
            builder.try_push(client.initialize_market_config_buffer(
                store,
                &buffer_keypair,
                expire_after.as_secs().try_into().unwrap_or(u32::MAX),
            ))?;
            buffer_keypair.pubkey()
        };

        tracing::info!("Buffer account to be pushed to: {buffer}");

        let configs = self.0.iter().collect::<Vec<_>>();
        for batch in configs.chunks(batch.get()) {
            builder.try_push(client.push_to_market_config_buffer(
                &buffer,
                batch.iter().map(|(key, value)| (key, value.0)),
            ))?;
        }

        crate::utils::send_or_serialize_transactions(
            builder,
            serialize_only,
            skip_preflight,
            |signatures, error| {
                println!("{signatures:#?}");
                match error {
                    None => Ok(()),
                    Some(err) => Err(err),
                }
            },
        )
        .await?;
        Ok(())
    }
}

/// Market Config with enable option.
#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct MarketConfig {
    #[serde(default)]
    enable: Option<bool>,
    #[serde(default)]
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    buffer: Option<Pubkey>,
    #[serde(flatten)]
    config: MarketConfigMap,
}

impl MarketConfigs {
    #[allow(clippy::too_many_arguments)]
    async fn update(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
        skip_preflight: bool,
        max_transaction_size: Option<usize>,
        receiver: Option<&Pubkey>,
        close_buffers: bool,
    ) -> gmsol::Result<()> {
        let mut builder = TransactionBuilder::new_with_options(
            client.store_program().solana_rpc(),
            false,
            max_transaction_size,
        );

        let mut buffers_to_close = HashSet::<Pubkey>::default();
        for (market_token, config) in &self.configs {
            if let Some(buffer) = &config.buffer {
                let buffer_account = client
                    .account::<MarketConfigBuffer>(buffer)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?;
                if buffer_account.store != *store {
                    return Err(gmsol::Error::invalid_argument(
                        "The provided buffer account is owned by different store",
                    ));
                }
                if buffer_account.authority != client.payer() {
                    return Err(gmsol::Error::invalid_argument(
                        "The authority of the provided buffer account is not the payer",
                    ));
                }
                tracing::info!("A buffer account is provided, it will be used first to update the market config. Add instruction to update `{market_token}` with it");
                builder.try_push(client.update_market_config_with_buffer(
                    store,
                    market_token,
                    buffer,
                ))?;
                if close_buffers {
                    buffers_to_close.insert(*buffer);
                }
            }
            for (key, value) in &config.config.0 {
                tracing::info!(%market_token, "Add instruction to update `{key}` to `{value}`");
                builder.try_push(client.update_market_config_by_key(
                    store,
                    market_token,
                    *key,
                    &value.0,
                )?)?;
            }
            if let Some(enable) = config.enable {
                tracing::info!(%market_token,
                    "Add instruction to {} market",
                    if enable { "enable" } else { "disable" },
                );
                builder.try_push(client.toggle_market(store, market_token, enable))?;
            }
        }

        // Push close buffer instructions.
        for buffer in buffers_to_close.iter() {
            builder.try_push(client.close_marekt_config_buffer(buffer, receiver))?;
        }

        crate::utils::send_or_serialize_transactions(
            builder,
            serialize_only,
            skip_preflight,
            |signatures, error| {
                println!("{signatures:#?}");
                match error {
                    None => Ok(()),
                    Some(err) => Err(err),
                }
            },
        )
        .await?;
        Ok(())
    }
}

fn toml_from_file<T>(path: &impl AsRef<Path>) -> gmsol::Result<T>
where
    T: DeserializeOwned,
{
    let mut buffer = String::new();
    std::fs::File::open(path)?.read_to_string(&mut buffer)?;
    toml::from_str(&buffer).map_err(gmsol::Error::invalid_argument)
}
