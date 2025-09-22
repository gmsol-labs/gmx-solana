use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

use anchor_spl::associated_token::get_associated_token_address;
use either::Either;
use eyre::OptionExt;
use gmsol_sdk::{
    client::{StoreFilter, DISC_OFFSET},
    core::{
        config::FactorKey,
        market::{MarketConfigFlag, VirtualInventoryFlag},
        oracle::PriceProviderKind,
        token_config::{
            TokenMapAccess, UpdateTokenConfigParams, DEFAULT_HEARTBEAT_DURATION, DEFAULT_PRECISION,
            DEFAULT_TIMESTAMP_ADJUSTMENT,
        },
    },
    ops::{
        token_config::UpdateFeedConfig, ConfigOps, GtOps, MarketOps, OracleOps, StoreOps,
        TokenAccountOps, TokenConfigOps, VirtualInventoryOps,
    },
    pda::find_virtual_inventory_for_swaps_address,
    programs::{
        anchor_lang::prelude::Pubkey,
        bytemuck,
        gmsol_store::accounts::{MarketConfigBuffer, VirtualInventory},
    },
    serde::{
        serde_market::{SerdeMarket, SerdeMarketConfig, SerdeMarketConfigBuffer},
        serde_token_map::SerdeTokenConfig,
        StringPubkey,
    },
    solana_utils::{
        bundle_builder::{BundleBuilder, BundleOptions},
        signer::LocalSignerRef,
        solana_client::rpc_filter::{Memcmp, RpcFilterType},
        solana_sdk::{signature::Keypair, signer::Signer},
    },
    utils::{market::MarketDecimals, zero_copy::ZeroCopy, Amount, Value},
};
use indexmap::{IndexMap, IndexSet};
use rust_decimal::Decimal;

use crate::{
    commands::{exchange::display_options_for_markets, utils::toml_from_file},
    config::DisplayOptions,
};

use super::{
    utils::{KeypairArgs, Side, ToggleValue},
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
    /// Display the content of the given market config buffer.
    Buffer {
        address: Pubkey,
        /// The expected market token to use for this buffer.
        #[arg(long)]
        market_token: Pubkey,
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
    /// Update feed configs from file.
    UpdateFeedConfig {
        #[arg(long)]
        token_map: Option<Pubkey>,
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
    /// Toggle the token price adjustment for the given token.
    ToggleTokenPriceAdjustment {
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
        enable: bool,
    },
    /// Toggle market.
    ToggleMarket {
        market_token: Pubkey,
        #[command(flatten)]
        toggle: ToggleValue,
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
    /// Update Market Config Flag.
    ToggleConfigFlag {
        /// The market token of the market to update.
        market_token: Pubkey,
        /// The config key to update.
        #[arg(long)]
        key: MarketConfigFlag,
        /// The boolean value that the flag to update to.
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Update Market Configs from file.
    UpdateConfigs {
        path: PathBuf,
        /// Receiver for the buffer's lamports.
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Whether to keep the used market config buffer accounts.
        #[arg(long)]
        keep_buffers: bool,
    },
    /// Toggle GT minting.
    ToggleGtMinting {
        #[arg(required = true, num_args = 1..)]
        market_tokens: Vec<Pubkey>,
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Initialize GT.
    InitGt {
        #[arg(long, short, default_value_t = 7)]
        decimals: u8,
        #[arg(long, short = 'c', default_value_t = Value(Decimal::new(1, 2)))]
        initial_minting_cost: Value,
        #[arg(long, default_value_t = Value(Decimal::new(1_021, 3)))]
        grow_factor: Value,
        #[arg(long, default_value_t = Amount(Decimal::new(210_000, 0)))]
        grow_step: Amount,
        #[arg(required = true)]
        ranks: Vec<Amount>,
    },
    /// Set order fee discount factors.
    SetOrderFeeDiscountFactors {
        #[arg(required = true)]
        factors: Vec<Value>,
    },
    /// Set referral reward factors.
    SetReferralRewardFactors {
        #[arg(required = true)]
        factors: Vec<Value>,
    },
    /// Set referred discount.
    SetReferredDiscountFactor { factor: Value },
    /// Create or update token metadata from file.
    UpdateTokenMetadatas { path: PathBuf },
    /// Display virtual inventories.
    VirtualInventories {
        address: Option<Pubkey>,
        /// Displays the market list associated with the given VI.
        #[arg(long, requires = "address")]
        markets: bool,
    },
    /// Create a virtual inventory for swaps.
    CreateVirtualInventoryForSwaps {
        #[arg(long)]
        index: u32,
        #[arg(long, short)]
        long_amount_decimals: u8,
        #[arg(long, short)]
        short_amount_decimals: u8,
    },
    /// Create a virtual inventory for positions.
    CreateVirtualInventoryForPositions { index_token: Pubkey },
    /// Disable virtual inventories.
    DisableVirtualInventories {
        #[arg(required = true, num_args = 1..)]
        addresses: Vec<Pubkey>,
    },
    /// Join the given virtual inventory for swaps.
    JoinVirtualInventoryForSwaps {
        #[arg(required = true, num_args = 1..)]
        market_tokens: Vec<Pubkey>,
        #[arg(long)]
        virtual_inventory: Pubkey,
    },
    /// Join the given virtual inventory for positions.
    JoinVirtualInventoryForPositions {
        #[arg(required = true, num_args = 1..)]
        market_tokens: Vec<Pubkey>,
        #[arg(long)]
        virtual_inventory: Pubkey,
    },
    /// Leave the given virtual inventory.
    LeaveVirtualInventory {
        #[arg(required = true, num_args = 1..)]
        market_tokens: Vec<Pubkey>,
        #[arg(long)]
        virtual_inventory: Pubkey,
    },
    /// Close virtual inventories.
    CloseVirtualInventories {
        #[arg(required = true, num_args = 1..)]
        addresses: Vec<Pubkey>,
    },
    /// Display updatable market config flags and/or factors.
    /// By default, show both. Use --without-flags or --without-factors to hide.
    UpdatableConfig {
        #[arg(long, alias = "no-flags")]
        without_flags: bool,
        #[arg(long, alias = "no-factors")]
        without_factors: bool,
    },
    /// Query if a specific market config flag or factor is updatable.
    IsConfigUpdatable {
        #[arg(long, group = "what")]
        flag: Option<MarketConfigFlag>,
        #[arg(long, group = "what")]
        factor: Option<gmsol_sdk::core::market::MarketConfigKey>,
    },
    /// Enable or disable updates for a set of market config flags and/or factors.
    SetConfigUpdatable {
        /// Flags to set updatable.
        #[arg(long = "flag", required = false, num_args = 1..)]
        flags: Vec<MarketConfigFlag>,
        /// Factors to set updatable.
        #[arg(long = "factor", required = false, num_args = 1..)]
        factors: Vec<gmsol_sdk::core::market::MarketConfigKey>,
        #[command(flatten)]
        toggle: ToggleValue,
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
            Command::Buffer {
                address,
                market_token,
            } => {
                let buffer = client
                    .account::<MarketConfigBuffer>(address)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?;
                let token_map = client.authorized_token_map(store).await?;
                let market = client.market_by_token(store, market_token).await?;
                let decimals = MarketDecimals::new(&market.meta.into(), &token_map)?;
                let buffer = SerdeMarketConfigBuffer::from_market_config_buffer(&buffer, decimals)?;
                println!(
                    "{}",
                    output.display_keyed_account(
                        address,
                        &buffer,
                        DisplayOptions::table_projection([
                            ("pubkey", "Address"),
                            ("store", "Store"),
                            ("authority", "Authority"),
                            ("expiry", "Expiry"),
                        ])
                    )?
                );
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
            Command::UpdateFeedConfig { token_map, path } => {
                let token_map = token_map_address(client, token_map.as_ref()).await?;
                let configs: IndexMap<StringPubkey, IndexMap<PriceProviderKind, UpdateFeedConfig>> =
                    toml_from_file(path)?;
                let mut bundle = client.bundle_with_options(options);
                for (token, feeds) in configs {
                    for (provider, config) in feeds {
                        let rpc = client
                            .update_feed_config(store, &token_map, &token, provider, config)?;
                        bundle.push(rpc)?;
                    }
                }
                bundle
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
            Command::ToggleTokenPriceAdjustment {
                token,
                token_map,
                toggle,
            } => {
                let token_map_address = token_map_address(client, token_map.as_ref()).await?;
                client
                    .toggle_token_price_adjustment(
                        store,
                        &token_map_address,
                        token,
                        toggle.is_enable(),
                    )
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
                let configs: MarketConfigs = toml_from_file(path)?;
                let config = configs
                    .configs
                    .get(market_token)
                    .ok_or_eyre(format!("the config for `{market_token}` not found"))?;
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
                    &config.config,
                    expire_after,
                    *batch,
                    options,
                )
                .await?;
                client.send_or_serialize(bundle).await?;
                return Ok(());
            }
            Command::CreateVault { token } => {
                let (rpc, vault) = client.initialize_market_vault(store, token);
                println!("Market Vault: {vault}");
                rpc.into_bundle_with_options(options)?
            }
            Command::CreateMarket {
                name,
                index_token,
                long_token,
                short_token,
                enable,
            } => {
                let (rpc, market_token) = client
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
                println!("Market Token: {market_token}");
                rpc.into_bundle_with_options(options)?
            }
            Command::CreateMarkets { path, enable } => {
                let markets: IndexMap<String, CreateMarket> = toml_from_file(path)?;
                create_markets(client, *enable, &markets, options).await?
            }
            Command::ToggleMarket {
                market_token,
                toggle,
            } => client
                .toggle_market(store, market_token, toggle.is_enable())
                .into_bundle_with_options(options)?,
            Command::FundMarket {
                market_token,
                side,
                amount,
            } => {
                let market = client.market_by_token(store, market_token).await?;
                let token = match side {
                    Side::Long => market.meta.long_token_mint,
                    Side::Short => market.meta.short_token_mint,
                };
                let source_account = get_associated_token_address(&client.payer(), &token);
                client
                    .fund_market(store, market_token, &source_account, *amount, Some(&token))
                    .await?
                    .into_bundle_with_options(options)?
            }
            Command::ToggleConfigFlag {
                market_token,
                key,
                toggle,
            } => client
                .update_market_config_flag_by_key(store, market_token, *key, toggle.is_enable())?
                .into_bundle_with_options(options)?,
            Command::UpdateConfigs {
                path,
                receiver,
                keep_buffers,
            } => {
                let configs: MarketConfigs = toml_from_file(path)?;
                configs
                    .update_market_configs(client, receiver.as_ref(), !*keep_buffers, options)
                    .await?
            }
            Command::ToggleGtMinting {
                market_tokens,
                toggle,
            } => {
                let mut bundle = client.bundle_with_options(options);
                for market_token in market_tokens {
                    let rpc = client.toggle_gt_minting(store, market_token, toggle.is_enable());
                    bundle.push(rpc)?;
                }
                bundle
            }
            Command::SetConfigUpdatable {
                flags,
                factors,
                toggle,
            } => {
                use indexmap::IndexMap;
                let value = toggle.is_enable();
                let flags_map: IndexMap<_, _> = flags.iter().cloned().map(|f| (f, value)).collect();
                let factors_map: IndexMap<_, _> = factors
                    .iter()
                    .map(|k| Ok::<_, eyre::Report>((*k).try_into().map(|f| (f, value))?))
                    .collect::<eyre::Result<_>>()?;
                client
                    .set_market_config_updatable(store, flags_map, factors_map)?
                    .into_bundle_with_options(options)?
            }
            Command::InitGt {
                decimals,
                initial_minting_cost,
                grow_factor,
                grow_step,
                ranks,
            } => {
                debug_assert!(!ranks.is_empty());
                let decimals = *decimals;
                let ranks = ranks
                    .iter()
                    .map(|a| Ok(a.to_u64(decimals)?))
                    .collect::<eyre::Result<Vec<_>>>()?;
                if !ranks.is_sorted() {
                    eyre::bail!("ranks must be sorted");
                }
                let initial_minting_cost =
                    initial_minting_cost.to_u128()? / 10u128.pow(decimals.into());
                let grow_factor = grow_factor.to_u128()?;
                let grow_step = grow_step.to_u64(decimals)?;
                client
                    .initialize_gt(
                        store,
                        decimals,
                        initial_minting_cost,
                        grow_factor,
                        grow_step,
                        ranks,
                    )
                    .into_bundle_with_options(options)?
            }
            Command::SetOrderFeeDiscountFactors { factors } => {
                debug_assert!(!factors.is_empty());
                let factors = factors
                    .iter()
                    .map(|v| Ok(v.to_u128()?))
                    .collect::<eyre::Result<Vec<_>>>()?;
                client
                    .gt_set_order_fee_discount_factors(store, factors)
                    .into_bundle_with_options(options)?
            }
            Command::SetReferralRewardFactors { factors } => {
                debug_assert!(!factors.is_empty());
                let factors = factors
                    .iter()
                    .map(|v| Ok(v.to_u128()?))
                    .collect::<eyre::Result<Vec<_>>>()?;
                client
                    .gt_set_referral_reward_factors(store, factors)
                    .into_bundle_with_options(options)?
            }
            Command::SetReferredDiscountFactor { factor } => client
                .insert_global_factor_by_key(
                    store,
                    FactorKey::OrderFeeDiscountForReferredUser,
                    &factor.to_u128()?,
                )
                .into_bundle_with_options(options)?,
            Command::UpdateTokenMetadatas { path } => {
                let config: TokenMetadatas = toml_from_file(path)?;
                let mut bundle = client.bundle_with_options(options);
                for (mint, metadata) in config.0 {
                    let rpc = if metadata.init {
                        let (rpc, token_metadata) = client
                            .create_token_metadata(
                                store,
                                &mint,
                                metadata.name,
                                metadata.symbol,
                                metadata.uri,
                            )
                            .swap_output(());
                        println!("Creating token metadata {token_metadata} for {mint}");
                        rpc
                    } else {
                        client.update_token_metadata_by_mint(
                            store,
                            &mint,
                            metadata.name,
                            metadata.symbol,
                            metadata.uri,
                        )
                    };
                    bundle.push(rpc)?;
                }

                bundle
            }
            Command::VirtualInventories { address, markets } => {
                use gmsol_sdk::programs::gmsol_store::accounts::Market as MarketAccount;

                match address {
                    Some(address) => {
                        let vi = client
                            .account::<ZeroCopy<VirtualInventory>>(address)
                            .await?
                            .ok_or(gmsol_sdk::Error::NotFound)?;
                        let vi = SerdeVirtualInventory::new(address, &vi.0)?;
                        if *markets {
                            let offset = if vi.for_swaps {
                                bytemuck::offset_of!(MarketAccount, virtual_inventory_for_swaps)
                            } else {
                                bytemuck::offset_of!(MarketAccount, virtual_inventory_for_positions)
                            };
                            let token_map = client.authorized_token_map(store).await?;
                            let markets = client
                                .store_accounts::<ZeroCopy<MarketAccount>>(
                                    Some(StoreFilter::new(
                                        store,
                                        bytemuck::offset_of!(MarketAccount, store),
                                    )),
                                    Some(RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                                        DISC_OFFSET + offset,
                                        address.as_ref(),
                                    ))),
                                )
                                .await?;
                            let mut serde_markets = markets
                                .iter()
                                .map(|(p, m)| {
                                    SerdeMarket::from_market(&m.0, &token_map).map(|m| (p, m))
                                })
                                .collect::<gmsol_sdk::Result<Vec<(_, _)>>>()?;
                            serde_markets.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
                            serde_markets.sort_by_key(|(_, m)| m.enabled);
                            println!(
                                "{}",
                                output.display_keyed_accounts(
                                    serde_markets,
                                    display_options_for_markets(),
                                )?
                            );
                        } else {
                            let msg =
                                output.display_keyed_account(address, &vi, Default::default())?;
                            println!("{msg}");
                        }
                    }
                    None => {
                        let vis = client
                            .store_accounts::<ZeroCopy<VirtualInventory>>(
                                Some(StoreFilter::new(
                                    store,
                                    bytemuck::offset_of!(VirtualInventory, store),
                                )),
                                None,
                            )
                            .await?;
                        let vis = vis
                            .iter()
                            .map(|(pubkey, vi)| {
                                Ok((pubkey, SerdeVirtualInventory::new(pubkey, &vi.0)?))
                            })
                            .collect::<gmsol_sdk::Result<BTreeMap<_, _>>>()?;
                        let msg = output.display_keyed_accounts(vis, Default::default())?;
                        println!("{msg}");
                    }
                }
                return Ok(());
            }
            Command::UpdatableConfig {
                without_flags,
                without_factors,
            } => {
                use gmsol_sdk::core::market::{
                    MarketConfigFactor, MarketConfigFlag, MarketConfigKey,
                };
                use gmsol_sdk::programs::gmsol_store::accounts as store_accounts;
                use strum::IntoEnumIterator;

                let store_account: std::sync::Arc<store_accounts::Store> =
                    client.store(store).await?;

                if !*without_flags {
                    let mut rows = Vec::new();
                    for flag in MarketConfigFlag::iter() {
                        let updatable = store_account
                            .market_config_permissions
                            .updatable_market_config_flags
                            .get_flag(flag);
                        rows.push(serde_json::json!({
                            "name": flag.to_string(),
                            "updatable": updatable,
                        }));
                    }
                    let out = output.display_many(
                        rows,
                        DisplayOptions::table_projection([
                            ("name", "Flag"),
                            ("updatable", "Updatable"),
                        ]),
                    )?;
                    println!("{out}");
                }

                if !*without_factors {
                    let mut rows = Vec::new();
                    for key in MarketConfigKey::iter() {
                        if let Ok(factor) = MarketConfigFactor::try_from(key) {
                            let updatable = store_account
                                .market_config_permissions
                                .updatable_market_config_factors
                                .get_flag(factor);
                            rows.push(serde_json::json!({
                                "key": key.to_string(),
                                "updatable": updatable,
                            }));
                        }
                    }
                    let out = output.display_many(
                        rows,
                        DisplayOptions::table_projection([
                            ("key", "Factor"),
                            ("updatable", "Updatable"),
                        ]),
                    )?;
                    println!("{out}");
                }

                return Ok(());
            }
            Command::IsConfigUpdatable { flag, factor } => {
                use gmsol_sdk::core::market::MarketConfigFactor;
                use gmsol_sdk::programs::gmsol_store::accounts as store_accounts;

                let store_account: std::sync::Arc<store_accounts::Store> =
                    client.store(store).await?;
                match (flag, factor) {
                    (Some(flag), None) => {
                        let is_updatable = store_account
                            .market_config_permissions
                            .updatable_market_config_flags
                            .get_flag(*flag);
                        println!("{is_updatable}");
                    }
                    (None, Some(key)) => {
                        let factor = MarketConfigFactor::try_from(*key)?;
                        let is_updatable = store_account
                            .market_config_permissions
                            .updatable_market_config_factors
                            .get_flag(factor);
                        println!("{is_updatable}");
                    }
                    _ => {
                        eyre::bail!("specify exactly one of --flag or --factor");
                    }
                }
                return Ok(());
            }
            Command::CreateVirtualInventoryForSwaps {
                index,
                long_amount_decimals,
                short_amount_decimals,
            } => {
                let (rpc, vi) = client
                    .create_virtual_inventory_for_swaps(
                        store,
                        *index,
                        *long_amount_decimals,
                        *short_amount_decimals,
                    )?
                    .swap_output(());
                println!("{vi}");
                rpc.into_bundle_with_options(options)?
            }
            Command::CreateVirtualInventoryForPositions { index_token } => {
                let (rpc, vi) = client
                    .create_virtual_inventory_for_positions(store, index_token)?
                    .swap_output(());
                println!("{vi}");
                rpc.into_bundle_with_options(options)?
            }
            Command::DisableVirtualInventories { addresses } => {
                let mut bundle = client.bundle_with_options(options);
                for address in addresses {
                    let rpc = client.disable_virtual_inventory(store, address)?;
                    bundle.push(rpc)?;
                }
                bundle
            }
            Command::JoinVirtualInventoryForSwaps {
                market_tokens,
                virtual_inventory,
            } => {
                let mut bundle = client.bundle_with_options(options);
                let token_map = client
                    .authorized_token_map_address(store)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?;
                for market_token in market_tokens {
                    let market = client.find_market_address(store, market_token);
                    let rpc = client
                        .join_virtual_inventory_for_swaps(
                            store,
                            &market,
                            virtual_inventory,
                            Some(&token_map),
                        )
                        .await?;
                    bundle.push(rpc)?;
                }
                bundle
            }
            Command::JoinVirtualInventoryForPositions {
                market_tokens,
                virtual_inventory,
            } => {
                let mut bundle = client.bundle_with_options(options);
                for market_token in market_tokens {
                    let market = client.find_market_address(store, market_token);
                    let rpc = client.join_virtual_inventory_for_positions(
                        store,
                        &market,
                        virtual_inventory,
                    )?;
                    bundle.push(rpc)?;
                }
                bundle
            }
            Command::LeaveVirtualInventory {
                market_tokens,
                virtual_inventory,
            } => {
                let mut bundle = client.bundle_with_options(options);
                let vi = client
                    .account::<ZeroCopy<VirtualInventory>>(virtual_inventory)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?
                    .0;
                let markets = market_tokens
                    .iter()
                    .map(|token| client.find_market_address(store, token))
                    .collect::<Vec<_>>();
                if vi.flags.get_flag(VirtualInventoryFlag::Disabled) {
                    for market in &markets {
                        bundle.push(client.leave_disabled_virtual_inventory(
                            store,
                            market,
                            virtual_inventory,
                        )?)?;
                    }
                } else {
                    let first = markets.first().expect("must exist");
                    let market = client.market(first).await?;
                    if market.virtual_inventory_for_swaps == *virtual_inventory {
                        for market in &markets {
                            bundle.push(client.leave_virtual_inventory_for_swaps(
                                store,
                                market,
                                virtual_inventory,
                            )?)?;
                        }
                    } else if market.virtual_inventory_for_positions == *virtual_inventory {
                        for market in &markets {
                            bundle.push(client.leave_virtual_inventory_for_positions(
                                store,
                                market,
                                virtual_inventory,
                            )?)?;
                        }
                    } else {
                        eyre::bail!("the first market has not included this virtual inventory.");
                    }
                }
                bundle
            }
            Command::CloseVirtualInventories { addresses } => {
                let mut bundle = client.bundle_with_options(options);
                for address in addresses {
                    bundle.push(client.close_virtual_inventory_account(store, address)?)?;
                }
                bundle
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

impl MarketConfigs {
    async fn update_market_configs<'a>(
        &self,
        client: &'a CommandClient,
        receiver: Option<&Pubkey>,
        close_buffers: bool,
        options: BundleOptions,
    ) -> eyre::Result<BundleBuilder<'a, LocalSignerRef>> {
        let store = &client.store;
        let token_map = client.authorized_token_map(store).await?;
        let mut bundle = client.bundle_with_options(options);

        let mut buffers_to_close = IndexSet::<Pubkey>::default();

        for (market_token, config) in &self.configs {
            let market = client.market_by_token(store, market_token).await?;
            let decimals = MarketDecimals::new(&market.meta.into(), &token_map)?;
            if let Some(buffer) = &config.buffer {
                let buffer_account = client
                    .account::<MarketConfigBuffer>(buffer)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?;
                if buffer_account.store != *store {
                    return Err(gmsol_sdk::Error::custom(
                        "The provided buffer account is owned by different store",
                    )
                    .into());
                }
                if buffer_account.authority != client.payer() {
                    return Err(gmsol_sdk::Error::custom(
                        "The authority of the provided buffer account is not the payer",
                    )
                    .into());
                }
                tracing::info!("A buffer account is provided, it will be used first to update the market config. Add instruction to update `{market_token}` with it");
                bundle.push(client.update_market_config_with_buffer(
                    store,
                    market_token,
                    buffer,
                ))?;
                if close_buffers {
                    buffers_to_close.insert(**buffer);
                }
            }
            for (key, value) in &config.config.0 {
                let value = value.to_u128(decimals.market_config_decimals(*key)?)?;
                tracing::info!(%market_token, "Add instruction to update `{key}` to `{value}`");
                bundle.push(client.update_market_config_by_key(
                    store,
                    market_token,
                    *key,
                    &value,
                )?)?;
            }
            if let Some(enable) = config.enable {
                tracing::info!(%market_token,
                    "Add instruction to {} market",
                    if enable { "enable" } else { "disable" },
                );
                bundle.push(client.toggle_market(store, market_token, enable))?;
            }
        }

        // Push close buffer instructions.
        for buffer in buffers_to_close.iter() {
            bundle.push(client.close_marekt_config_buffer(buffer, receiver))?;
        }

        Ok(bundle)
    }
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
        let Some(pyth_feed_id) = self.pyth_feed_id.as_ref() else {
            return Ok(None);
        };
        let feed_id_as_key = Pubkey::new_from_array(parse_hex_encoded_feed_id(pyth_feed_id)?);
        Ok(Some(feed_id_as_key))
    }

    fn chainlink_data_streams_feed_id(&self) -> eyre::Result<Option<Pubkey>> {
        let Some(feed_id) = self.chainlink_data_streams_feed_id.as_ref() else {
            return Ok(None);
        };

        let feed_id_as_key = Pubkey::new_from_array(parse_hex_encoded_feed_id(feed_id)?);
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CreateMarket {
    index_token: StringPubkey,
    long_token: StringPubkey,
    short_token: StringPubkey,
}

async fn create_markets<'a>(
    client: &'a CommandClient,
    enable: bool,
    markets: &IndexMap<String, CreateMarket>,
    options: BundleOptions,
) -> eyre::Result<BundleBuilder<'a, LocalSignerRef>> {
    let store = &client.store;
    let mut bundle = client.bundle_with_options(options);
    let token_map = token_map_address(client, None).await?;
    let mut tokens = IndexMap::with_capacity(markets.len());
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
        tokens.insert(name, token);
        bundle.push(rpc)?;
    }

    for (name, token) in tokens {
        println!("{name}: {token}");
    }

    Ok(bundle)
}

fn parse_hex_encoded_feed_id(feed_id: &str) -> eyre::Result<[u8; 32]> {
    let feed_id = feed_id.strip_prefix("0x").unwrap_or(feed_id);

    let mut bytes = [0; 32];
    hex::decode_to_slice(feed_id, &mut bytes)?;

    Ok(bytes)
}

#[derive(Debug, serde::Deserialize)]
struct TokenMetadatas(IndexMap<StringPubkey, TokenMetadata>);

#[derive(Debug, serde::Deserialize)]
struct TokenMetadata {
    name: String,
    symbol: String,
    uri: String,
    #[serde(default)]
    init: bool,
}

#[derive(Debug, serde::Serialize)]
struct SerdeVirtualInventory {
    index: u32,
    long_amount: Amount,
    short_amount: Amount,
    is_enabled: bool,
    ref_count: u32,
    long_decimals: u8,
    short_decimals: u8,
    for_swaps: bool,
}

impl SerdeVirtualInventory {
    fn new(address: &Pubkey, vi: &VirtualInventory) -> gmsol_sdk::Result<Self> {
        use gmsol_sdk::programs::gmsol_store::ID;

        let pool = &vi.pool.pool;
        let for_swaps =
            find_virtual_inventory_for_swaps_address(&vi.store, vi.index, &ID).0 == *address;
        let is_enabled = !vi.flags.get_flag(VirtualInventoryFlag::Disabled);
        Ok(Self {
            index: vi.index,
            long_amount: Amount::from_u128(pool.long_token_amount, vi.long_amount_decimals)?,
            short_amount: Amount::from_u128(pool.short_token_amount, vi.short_amount_decimals)?,
            is_enabled,
            ref_count: vi.ref_count,
            long_decimals: vi.long_amount_decimals,
            short_decimals: vi.short_amount_decimals,
            for_swaps,
        })
    }
}
