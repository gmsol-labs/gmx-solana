use std::{collections::BTreeMap, num::NonZeroUsize};

use anchor_client::{
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::pubkey::Pubkey,
};
use eyre::OptionExt;
use futures_util::{pin_mut, StreamExt};
use gmsol::{
    constants,
    pyth::Hermes,
    store::gt::current_time_window_index,
    types::{
        self,
        common::action::Action,
        feature::{display_feature, ActionDisabledFlag, DomainDisabledFlag},
        user::ReferralCodeV2,
        PriceFeed, TokenMapAccess,
    },
    utils::{unsigned_amount_to_decimal, unsigned_value_to_decimal, ZeroCopy},
};
use gmsol_model::{
    price::{Price, Prices},
    Balance, BalanceExt, ClockKind, PnlFactorKind, PoolKind, PositionExt, PositionStateExt,
};
use gmsol_solana_utils::utils::inspect_transaction;
use gmsol_store::states::{
    self, AddressKey, AmountKey, FactorKey, MarketConfigKey, PriceProviderKind,
};
use gmsol_timelock::states::InstructionAccess;
use indexmap::IndexMap;
use num_format::{Locale, ToFormattedString};
use prettytable::{row, Table};
use pyth_sdk::Identifier;
use strum::IntoEnumIterator;

use crate::{
    ser::{self, SerializeMarket},
    utils::{table_format, Output, SelectGtExchangeVaultByDate},
    GMSOLClient,
};

#[derive(clap::Args)]
pub(super) struct InspectArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// `User` account.
    User {
        #[arg(group = "select-user")]
        address: Option<Pubkey>,
        #[arg(long, short, group = "select-user")]
        owner: Option<Pubkey>,
        #[arg(long)]
        show_address: bool,
    },
    /// `ReferralCode` account.
    ReferralCode {
        #[arg(group = "select-code")]
        address: Option<Pubkey>,
        #[arg(long, short, group = "select-code")]
        code: Option<String>,
    },
    /// `Store` account.
    Store {
        #[arg(long, short, group = "other-store")]
        address: Option<Pubkey>,
        #[arg(long, short, group = "other-store")]
        key: Option<String>,
        #[arg(long)]
        show_address: bool,
        #[arg(long, group = "get")]
        debug: bool,
        #[arg(long, group = "get")]
        get_amount: Option<AmountKey>,
        #[arg(long, group = "get")]
        get_factor: Option<FactorKey>,
        #[arg(long, group = "get")]
        get_address: Option<AddressKey>,
        /// Ger roles for a member.
        #[arg(long, group = "get", value_name = "USER")]
        get_roles: Option<Pubkey>,
        /// Get members for the role.
        #[arg(long, group = "get", value_name = "ROLE")]
        get_members: Option<String>,
        /// Get all roles.
        #[arg(long, group = "get")]
        get_all_roles: bool,
        /// Get all members.
        #[arg(long, group = "get")]
        get_all_members: bool,
    },
    /// `TokenMap` account.
    TokenMap {
        address: Option<Pubkey>,
        #[arg(long, value_name = "TOKEN", group = "get-input")]
        get: Option<Pubkey>,
        /// Modify the get command to get the feed of the given provider.
        #[arg(long, value_name = "PROVIDER")]
        feed: Option<PriceProviderKind>,
        /// Get metadata
        #[arg(long, group = "get-input")]
        meta: bool,
        /// Output format.
        #[arg(long, short)]
        output: Option<Output>,
        #[arg(long)]
        debug: bool,
    },
    /// `Market` account.
    Market {
        /// Market token address.
        address: Option<Pubkey>,
        /// Consider the address as market address rather than the address of its market token.
        #[arg(long)]
        as_market_address: bool,
        #[arg(long)]
        get_config: Option<MarketConfigKey>,
        /// Output format.
        #[arg(long, short)]
        output: Option<Output>,
        #[arg(long)]
        debug: bool,
    },
    /// Market status.
    MarketStatus {
        /// Market token address.
        market_token: Pubkey,
        /// Prices.
        #[command(flatten)]
        prices: Option<MarketPrices>,
    },
    /// `MarketConfigBuffer` account.
    MarketConfigBuffer {
        address: Pubkey,
        #[arg(long)]
        debug: bool,
    },
    /// `Deposit` account.
    Deposit { address: Pubkey },
    /// `Withdrawal` account.
    Withdrawal { address: Pubkey },
    /// `Oracle` account.
    Oracle {
        #[arg(long, env)]
        oracle: Pubkey,
    },
    /// `Order` account.
    Order {
        address: Option<Pubkey>,
        #[arg(long, value_name = "LIMIT")]
        event: Option<Option<NonZeroUsize>>,
    },
    /// `Position` account.
    Position {
        address: Option<Pubkey>,
        #[arg(long, short)]
        market_token: Option<Pubkey>,
        /// Owner of the positions. Default to the connected wallet.
        #[arg(long, group = "owners")]
        owner: Option<Pubkey>,
        /// All user.
        #[arg(long, group = "owners")]
        all: bool,
        /// Whether to show the liquidatble positions only.
        #[arg(long, short, requires = "market_token")]
        liquidatable: bool,
        #[arg(long)]
        debug: bool,
        #[arg(long, short)]
        output: Option<Output>,
    },
    /// Watch Pyth Price Updates.
    WatchPyth {
        #[arg(required = true)]
        feed_ids: Vec<String>,
    },
    /// Get Pyth Price Updates.
    GetPyth {
        #[arg(required = true)]
        feed_ids: Vec<String>,
        #[arg(long)]
        post: bool,
    },
    /// Get chainlink feed updates.
    Chainlink {
        #[arg(long, short)]
        testnet: bool,
        #[arg(long, short)]
        decode: bool,
        #[arg(long, short)]
        watch: bool,
        #[arg(required = true)]
        feed_ids: Vec<String>,
    },
    /// Insepct controller.
    Features,
    /// Get the event authority address.
    EventAuthority,
    /// Generate Anchor Discriminator with the given name.
    Discriminator {
        name: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        raw: bool,
    },
    /// Inspect instruction data.
    IxData {
        /// Data.
        data: String,
        /// Program.
        #[arg(long, short, default_value = "store")]
        program: Program,
        // /// Output format.
        // #[arg(long, short)]
        // output: Option<Output>,
    },
    /// Inspect GLV.
    Glv {
        #[command(flatten)]
        select: SelectGlv,
    },
    /// GT exchange vault.
    GtExchangeVault { address: Option<Pubkey> },
    /// Price Feed.
    PriceFeed {
        #[arg(group = "price-feed-input")]
        address: Option<Pubkey>,
        #[arg(long, group = "price-feed-input")]
        authority: Option<Pubkey>,
    },
    /// Treasury.
    Treasury,
    /// GT Bank.
    GtBank {
        address: Option<Pubkey>,
        #[arg(long, value_name = "GT_EXCHANGE_VAULT")]
        vault: Option<Pubkey>,
        #[clap(flatten)]
        date: SelectGtExchangeVaultByDate,
        #[arg(long)]
        debug: bool,
    },
    /// Timelocked instruction.
    Tld {
        addresses: Vec<Pubkey>,
        #[clap(long)]
        raw: bool,
    },
    #[cfg(feature = "squads")]
    Squads {
        vault_transaction_address: Pubkey,
        #[clap(long)]
        raw: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Default)]
pub enum Program {
    #[default]
    /// Store.
    Store,
}

/// Market prices.
#[derive(clap::Args)]
pub struct MarketPrices {
    /// Index token price (unit price).
    #[arg(long, short)]
    pub index_price: u128,
    /// Long token price (unit price).
    #[arg(long, short)]
    pub long_price: u128,
    /// Short token price (unit price).
    #[arg(long, short)]
    pub short_price: u128,
}

#[derive(clap::Args)]
#[group(required = false)]
struct SelectGlv {
    address: Option<Pubkey>,
    #[arg(long, short)]
    index: Option<u16>,
    #[arg(long, short)]
    token: Option<Pubkey>,
}

impl MarketPrices {
    fn to_prices(&self) -> Prices<u128> {
        Prices {
            index_token_price: Price {
                min: self.index_price,
                max: self.index_price,
            },
            long_token_price: Price {
                min: self.long_price,
                max: self.long_price,
            },
            short_token_price: Price {
                min: self.short_price,
                max: self.short_price,
            },
        }
    }
}

impl InspectArgs {
    pub(super) async fn run(&self, client: &GMSOLClient, store: &Pubkey) -> gmsol::Result<()> {
        match &self.command {
            Command::Discriminator {
                name,
                namespace,
                raw,
            } => {
                println!(
                    "{:?}",
                    crate::utils::generate_discriminator(name, namespace.as_deref(), !*raw)
                );
            }
            Command::User {
                address,
                owner,
                show_address,
            } => {
                let address = address.unwrap_or_else(|| {
                    let owner = owner.unwrap_or_else(|| client.payer());
                    client.find_user_address(store, &owner)
                });
                let user = client.user(&address).await?;
                println!("{user:#?}");
                if *show_address {
                    println!("Address: {address}");
                }
            }
            Command::ReferralCode { address, code } => {
                let address = if let Some(address) = address {
                    *address
                } else if let Some(code) = code {
                    let code = ReferralCodeV2::decode(code)?;
                    client.find_referral_code_address(store, code)
                } else {
                    return Err(gmsol::Error::invalid_argument(
                        "must provide either `address` or `code`",
                    ));
                };
                let code = client
                    .account::<ZeroCopy<ReferralCodeV2>>(&address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0;
                println!("Code: {}", ReferralCodeV2::encode(&code.code, true));
                println!("Owner: {}", code.owner);
                println!("Next Owner: {}", code.next_owner());
            }
            Command::Store {
                address,
                key,
                show_address,
                debug,
                get_address,
                get_amount,
                get_factor,
                get_roles,
                get_members,
                get_all_roles,
                get_all_members,
            } => {
                let address = if let Some(address) = address {
                    *address
                } else if let Some(key) = key {
                    client.find_store_address(key)
                } else {
                    *store
                };
                let store = client.store(&address).await?;
                if let Some(key) = get_amount {
                    println!("{}", store.get_amount_by_key(*key));
                } else if let Some(key) = get_factor {
                    println!("{}", store.get_factor_by_key(*key));
                } else if let Some(key) = get_address {
                    println!("{}", store.get_address_by_key(*key));
                } else if let Some(user) = get_roles {
                    let role_store = store.role();
                    for role in role_store.roles() {
                        let name = role?;
                        if role_store.has_role(user, name)? {
                            println!("{name}");
                        }
                    }
                } else if let Some(role) = get_members {
                    let role_store = store.role();
                    for member in role_store.members() {
                        if role_store.has_role(&member, role)? {
                            println!("{member}");
                        }
                    }
                } else if *get_all_roles {
                    for role in store.role().roles() {
                        println!("{}", role.unwrap_or("*failed to parse role name*"));
                    }
                } else if *get_all_members {
                    let role_store = store.role();
                    let roles = role_store.roles().collect::<Result<Vec<_>, _>>()?;
                    for member in role_store.members() {
                        let roles = roles
                            .iter()
                            .filter_map(|role| match role_store.has_role(&member, role) {
                                Ok(true) => Some(Ok(*role)),
                                Ok(false) => None,
                                Err(err) => Some(Err(err)),
                            })
                            .collect::<Result<Vec<_>, _>>()?
                            .join("|");
                        println!("{member}, roles={roles}");
                    }
                } else if *debug {
                    println!("{store:?}");
                } else {
                    println!("{store}");
                }
                if *show_address {
                    println!("Address: {address}");
                }
            }
            Command::TokenMap {
                address,
                get,
                feed,
                meta,
                output,
                debug,
            } => {
                let mut authorized = true;
                let authorized_token_map = client.authorized_token_map_address(store).await?;
                let address = if let Some(address) = address {
                    if authorized_token_map != Some(*address) {
                        authorized = false;
                        tracing::warn!("this token map is not authorized by the store");
                    }
                    *address
                } else {
                    authorized_token_map.ok_or_else(|| {
                        gmsol::Error::invalid_argument("the token map of the store is not set")
                    })?
                };
                let output = output.unwrap_or_default();
                let token_map = client.token_map(&address).await?;
                if let Some(token) = get {
                    let config = token_map
                        .get(token)
                        .ok_or(gmsol::Error::invalid_argument("token not found"))?;
                    if let Some(kind) = feed {
                        let config = config.get_feed_config(kind)?;
                        let serialized = ser::SerializeFeedConfig::with_hint(kind, config);
                        output.print(&serialized, |serialized| {
                            if *debug {
                                Ok(format!("{config:#?}"))
                            } else {
                                Ok(serialized.formatted_feed_id())
                            }
                        })?;
                    } else {
                        let serialized = config.try_into()?;
                        output.print::<ser::SerializeTokenConfig>(&serialized, |serialized| {
                            if *debug {
                                Ok(format!("{config:#?}"))
                            } else {
                                format_token_config(serialized)
                            }
                        })?;
                    }
                } else if *meta {
                    output.print(
                        &serde_json::json!({
                            "address": address.to_string(),
                            "store": token_map.header().store.to_string(),
                            "tokens": token_map.header().len(),
                            "authorized": authorized,
                        }),
                        |_| {
                            if *debug {
                                Ok(format!("{:#?}", token_map.header()))
                            } else {
                                Ok(format!(
                                    "Address: {address}\nStore: {}\nTokens: {}\nAuthorized: {authorized}",
                                    token_map.header().store,
                                    token_map.header().len()
                                ))
                            }
                        },
                    )?;
                } else {
                    let map = token_map
                        .header()
                        .tokens()
                        .filter_map(|token| {
                            token_map
                                .get(&token)
                                .and_then(|config| ser::SerializeTokenConfig::try_from(config).ok())
                                .map(|config| (token.to_string(), config))
                        })
                        .collect::<IndexMap<_, _>>();
                    output.print(&map, |map| {
                        if *debug {
                            return Ok(format!("{token_map:#?}"));
                        }
                        let mut table = Table::new();
                        table.set_titles(row![
                            "Name",
                            "Token",
                            "",
                            "Enabled",
                            "Token Decimals",
                            "Price Precision",
                            "Expected Provider",
                        ]);
                        for (token, config) in map.iter() {
                            table.add_row(row![
                                &config.name,
                                token,
                                if config.synthetic { "(Synthetic*)" } else { "" },
                                config.enabled,
                                config.token_decimals,
                                config.price_precision,
                                config.expected_provider,
                            ]);
                        }
                        table.set_format(table_format());
                        Ok(table.to_string())
                    })?;
                }
            }
            Command::MarketStatus {
                market_token,
                prices,
            } => {
                let prices = prices
                    .as_ref()
                    .ok_or_eyre("currently prices must be provided")?
                    .to_prices();
                let status = client
                    .market_status(store, market_token, prices, true, false)
                    .await?;
                println!("{status:#?}");
                let market_token_price = client
                    .market_token_price(
                        store,
                        market_token,
                        prices,
                        PnlFactorKind::MaxAfterDeposit,
                        true,
                    )
                    .await?;
                println!(
                    "Market token price: {}",
                    gmsol::utils::unsigned_value_to_decimal(market_token_price)
                );
            }
            Command::Market {
                address,
                as_market_address,
                get_config,
                output,
                debug,
            } => {
                let output = output.unwrap_or_default();
                if let Some(mut address) = address {
                    if !as_market_address {
                        address = client.find_market_address(store, &address);
                    }
                    let market = client.market(&address).await?;
                    let serialized = SerializeMarket::from_market(&address, &market)?;
                    if let Some(key) = get_config {
                        let value = market.get_config_by_key(*key);
                        output.print(value, |value| Ok(value.to_string()))?;
                    } else if *debug {
                        println!("{market:#?}");
                    } else {
                        output.print(&serialized, format_market)?;
                    }
                } else {
                    let token_map = client.authorized_token_map(store).await?;
                    let markets = client
                        .markets(store)
                        .await?
                        .into_iter()
                        .filter_map(|(pubkey, market)| {
                            SerializeMarket::from_market(&pubkey, &market)
                                .inspect_err(
                                    |err| tracing::error!(%pubkey, %err, "parse market error"),
                                )
                                .ok()
                        })
                        .collect::<Vec<_>>();
                    output.print(&markets, |markets| {
                        use std::cmp::Reverse;

                        let mut table = Table::new();
                        let mut markets = markets
                            .iter()
                            .map(|market| {
                                let pools = &market.pools.0;
                                let oi =
                                    pools.get(&PoolKind::OpenInterestForLong).and_then(|long| {
                                        pools
                                            .get(&PoolKind::OpenInterestForShort)
                                            .map(|short| long.merge(short))
                                    });
                                (market, oi)
                            })
                            .collect::<Vec<_>>();
                        markets.sort_by_key(|(m, oi)| {
                            let oi = oi
                                .as_ref()
                                .map(|oi| {
                                    oi.long_amount().unwrap_or(0) + oi.short_amount().unwrap_or(0)
                                })
                                .unwrap_or_default();
                            (Reverse(m.enabled), Reverse(oi), &m.name)
                        });
                        for (market, oi) in markets {
                            let (oi_long, oi_short) = if let Some(oi) = oi {
                                (
                                    format!(
                                        "{}",
                                        unsigned_value_to_decimal(oi.long_amount()?).normalize()
                                    ),
                                    format!(
                                        "{}",
                                        unsigned_value_to_decimal(oi.short_amount()?).normalize()
                                    ),
                                )
                            } else {
                                ("*missing*".to_string(), "*missing*".to_string())
                            };
                            let name = &market.name;
                            let token = market.meta.market_token;
                            let enabled = market.enabled;
                            let mut claimable_fees = ("-".to_string(), "-".to_string());
                            if let Some(pool) = market.pools.0.get(&PoolKind::ClaimableFee) {
                                let (long, short) = (pool.long_amount()?, pool.short_amount()?);
                                if market.is_pure {
                                    let decimals = token_map
                                        .get(&market.meta.long_token)
                                        .expect("must exist")
                                        .token_decimals();
                                    claimable_fees.0 =
                                        unsigned_amount_to_decimal((long + short) as u64, decimals)
                                            .normalize()
                                            .to_string();
                                } else {
                                    let decimals = token_map
                                        .get(&market.meta.long_token)
                                        .expect("must exist")
                                        .token_decimals();
                                    claimable_fees.0 =
                                        unsigned_amount_to_decimal(long as u64, decimals)
                                            .normalize()
                                            .to_string();
                                    let decimals = token_map
                                        .get(&market.meta.short_token)
                                        .expect("must exist")
                                        .token_decimals();
                                    claimable_fees.1 =
                                        unsigned_amount_to_decimal(short as u64, decimals)
                                            .normalize()
                                            .to_string();
                                }
                            }
                            table.add_row(row![
                                name,
                                token,
                                enabled,
                                oi_long,
                                oi_short,
                                claimable_fees.0,
                                claimable_fees.1
                            ]);
                        }

                        table.set_titles(row![
                            "Name",
                            "Token",
                            "Enabled",
                            "OI Long ($)",
                            "OI Short ($)",
                            "Claimable fees (long)",
                            "Claimable fees (short)",
                        ]);
                        table.set_format(table_format());
                        Ok(table.to_string())
                    })?;
                }
            }
            Command::MarketConfigBuffer { address, debug } => {
                let buffer = client
                    .account::<states::market::config::MarketConfigBuffer>(address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?;
                if *debug {
                    println!("{buffer:#?}");
                } else {
                    println!("Authority: {}", buffer.authority);
                    println!("Store: {}", buffer.store);

                    // Format expiry.
                    let expiry = time::OffsetDateTime::from_unix_timestamp(buffer.expiry)
                        .map_err(gmsol::Error::unknown)?;
                    let now = time::OffsetDateTime::now_utc();
                    let msg = if expiry > now {
                        let dur = expiry - now;
                        format!(
                            "will expire in {}",
                            humantime::format_duration(
                                dur.try_into().map_err(gmsol::Error::unknown)?
                            )
                        )
                    } else {
                        let dur = now - expiry;
                        format!(
                            "expired {} ago",
                            humantime::format_duration(
                                dur.try_into().map_err(gmsol::Error::unknown)?
                            )
                        )
                    };
                    println!(
                        "Expiry: {} ({msg})",
                        humantime::format_rfc3339(expiry.into())
                    );

                    // Print configs.
                    if buffer.is_empty() {
                        println!("*buffer is empty*");
                    } else {
                        println!("Parameter count: {}", buffer.len());
                    }
                    let map = buffer
                        .iter()
                        .map(|entry| Ok((entry.key()?, entry.value())))
                        .collect::<Result<IndexMap<_, _>, gmsol::Error>>()?;
                    for (key, value) in map.iter() {
                        println!("{key} = {}", value.to_formatted_string(&Locale::en));
                    }
                }
            }
            Command::Deposit { address } => {
                println!("{:#?}", client.deposit(address).await?);
            }
            Command::Withdrawal { address } => {
                println!("{:#?}", client.withdrawal(address).await?);
            }
            Command::Features => {
                let store_address = store;
                let store = client.store(store_address).await?;
                println!("Store: {}", store_address);
                println!("Disabled features:");
                for domain in DomainDisabledFlag::iter() {
                    for action in ActionDisabledFlag::iter() {
                        if let Some(true) = store.get_feature_disabled(domain, action) {
                            println!("{}: disabled", display_feature(domain, action));
                        }
                    }
                }
            }
            Command::EventAuthority => {
                println!("DataStore: {}", client.store_program_id());
                println!("Event Authority: {}", client.store_event_authority());
            }
            Command::Oracle { oracle } => {
                println!(
                    "{:#?}",
                    client
                        .account::<states::Oracle>(oracle)
                        .await?
                        .ok_or(gmsol::Error::NotFound)
                );
            }
            Command::Order { address, event } => {
                if let Some(address) = address {
                    if let Some(limit) = *event {
                        let stream = match limit {
                            Some(limit) => client
                                .historical_store_cpi_events(address, None)
                                .await?
                                .take(limit.get())
                                .left_stream(),
                            None => client
                                .historical_store_cpi_events(address, None)
                                .await?
                                .right_stream(),
                        };
                        pin_mut!(stream);
                        while let Some(res) = stream.next().await {
                            match res {
                                Ok(events) => {
                                    println!("{events:#?}");
                                }
                                Err(err) => {
                                    tracing::error!(%err, "stream error");
                                }
                            }
                        }
                    } else {
                        println!("{:#?}", client.order(address).await?);
                    }
                } else {
                    let orders = client.orders(store, Some(&client.payer()), None).await?;
                    let mut table = Table::new();
                    table.set_titles(row!["Pubkey", "Market", "Order ID", "Order Kind", "Side"]);
                    table.set_format(table_format());
                    for (pubkey, order) in orders {
                        let is_long = if order.params().kind()?.is_swap() {
                            None
                        } else {
                            Some(order.params().side()?.is_long())
                        };
                        table.add_row(row![
                            pubkey,
                            truncate_pubkey(order.market_token()),
                            order.header().id(),
                            order.params().kind()?,
                            is_long
                                .map(|is_long| if is_long { "long" } else { "short" })
                                .unwrap_or("-"),
                        ]);
                    }
                    println!("{table}");
                }
            }
            Command::Position {
                address,
                market_token,
                owner,
                all,
                debug,
                liquidatable,
                output,
            } => {
                let output = output.unwrap_or_default();
                if let Some(address) = address {
                    let position = client.position(address).await?;
                    let serialized = ser::SerializePosition::try_from(&position)?;
                    output.print(&serialized, |serialized| {
                        if *debug {
                            return Ok(format!("{position:#?}"));
                        }
                        Ok(serialized.to_string())
                    })?;
                } else {
                    let owner = if *all {
                        None
                    } else {
                        Some(owner.unwrap_or(client.payer()))
                    };

                    let positions = client
                        .positions(store, owner.as_ref(), market_token.as_ref())
                        .await?;

                    let check_liquidatable = if let Some(market_token) =
                        (*liquidatable).then_some(()).and(market_token.as_ref())
                    {
                        let token_map = client.authorized_token_map(store).await?;
                        let market = client.find_market_address(store, market_token);
                        let market = client.market(&market).await?;
                        let hermes = Hermes::default();
                        let prices = hermes.unit_prices_for_market(&token_map, &*market).await?;
                        Some((market, prices))
                    } else {
                        None
                    };

                    let serialized = positions
                        .iter()
                        .filter(|(pubkey, p)| {
                            let Some((market, prices)) = check_liquidatable.as_ref() else {
                                return true;
                            };
                            // Filter the liquidatable positions.
                            if p.state.is_empty() {
                                return false;
                            }
                            let res = p
                                .as_position(market)
                                .map_err(gmsol::Error::from)
                                .and_then(|p| {
                                    tracing::debug!(?prices, "checking liquidatability");
                                    p.check_liquidatable(prices, true)
                                        .map_err(gmsol::Error::from)
                                })
                                .inspect_err(
                                    |err| tracing::error!(%err, %pubkey, "failed to check liquidatable"),
                                );
                            if let Ok(reason) = res {
                                reason.inspect(|reason| tracing::info!(%reason, %pubkey, "found liquidatable")).is_some()
                            } else {
                                false
                            }
                        })
                        .filter_map(|(pubkey, p)| {
                            let p = ser::SerializePosition::try_from(p)
                                .inspect_err(|err| {
                                    tracing::error!(%pubkey, %err, "serialize position error");
                                })
                                .ok()?;
                            Some((pubkey.to_string(), p))
                        })
                        .collect::<BTreeMap<_, _>>();
                    output.print(&serialized, |serialized| {
                        use std::cmp::Reverse;

                        if *debug {
                            return Ok(format!("{positions:#?}"));
                        }
                        let mut table = Table::new();
                        table.set_titles(row![
                            "Pubkey",
                            "Market",
                            "Collateral",
                            "Size ($)",
                            "Collateral Amount",
                            "Trade ID",
                        ]);
                        table.set_format(table_format());

                        let mut rows = serialized.iter().collect::<Vec<_>>();
                        rows.sort_by_key(|(_, p)| Reverse(p.state.size_in_usd));
                        rows.sort_by_key(|(_, p)| (p.market_token, p.collateral_token, !p.is_long));

                        for (pubkey, p) in rows {
                            let mut size =
                                unsigned_value_to_decimal(p.state.size_in_usd).normalize();
                            size.set_sign_positive(p.is_long);
                            table.add_row(row![
                                pubkey,
                                truncate_pubkey(&p.market_token),
                                truncate_pubkey(&p.collateral_token),
                                size,
                                p.state.collateral_amount.to_formatted_string(&Locale::en),
                                p.state.trade_id,
                            ]);
                        }

                        Ok(table.to_string())
                    })?;
                }
            }
            Command::WatchPyth { feed_ids } => {
                use futures_util::TryStreamExt;
                use gmsol::pyth::{EncodingType, Hermes};

                let hermes = Hermes::default();
                let feed_ids = parse_feed_ids(feed_ids)?;
                let stream = hermes
                    .price_updates(&feed_ids, Some(EncodingType::Base64))
                    .await?;
                futures_util::pin_mut!(stream);
                while let Some(update) = stream.try_next().await? {
                    tracing::info!("{:#?}", update.parsed());
                }
            }
            Command::GetPyth { feed_ids, post } => {
                use gmsol::pyth::{
                    EncodingType, Hermes, PythPullOracle, PythPullOracleContext, PythPullOracleOps,
                };

                let hermes = Hermes::default();
                let feed_ids = parse_feed_ids(feed_ids)?;
                let update = hermes
                    .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
                    .await?;
                tracing::info!("{:#?}", update.parsed());

                if *post {
                    let oracle = PythPullOracle::try_new(client)?;
                    let ctx = PythPullOracleContext::new(feed_ids);
                    let prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| {
                            for (feed_id, price_update) in prices {
                                tracing::info!(%feed_id, %price_update, "posting price update");
                            }
                            async { Ok(None) }
                        })
                        .await?;
                    match prices.send_all(None, true, true).await {
                        Ok(signatures) => {
                            tracing::info!("successfully sent all txs: {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, "sent txs error, successful list: {signatures:#?}");
                        }
                    }
                }
            }
            Command::Chainlink {
                testnet,
                feed_ids,
                decode,
                watch,
            } => {
                use gmsol::{chainlink::Client, types::PriceFeedPrice};
                use gmsol_chainlink_datastreams::report::Report;
                use time::OffsetDateTime;

                fn display_report(report: &Report) -> gmsol::Result<String> {
                    Ok(format!(
                        "{:#?}",
                        PriceFeedPrice::from_chainlink_report(report)?
                    ))
                }

                let client = if *testnet {
                    Client::from_testnet_defaults()?
                } else {
                    Client::from_defaults()?
                };

                let feed_ids = feed_ids.iter().map(|s| s.as_str());

                if *watch {
                    let stream = client.subscribe(feed_ids).await?;
                    futures_util::pin_mut!(stream);
                    while let Some(report) = stream.next().await {
                        match report {
                            Ok(report) => {
                                if *decode {
                                    println!("{}", display_report(&report.decode()?)?);
                                } else {
                                    println!("{report:#?}");
                                }
                            }
                            Err(err) => {
                                tracing::error!(%err, "receive error");
                            }
                        }
                    }
                } else {
                    let ts = OffsetDateTime::now_utc();
                    let reports = client.bulk_report(feed_ids, ts).await?;
                    for report in reports.iter() {
                        if *decode {
                            println!("{}", display_report(&report.decode()?)?);
                        } else {
                            println!("{report:#?}");
                        }
                    }
                }
            }
            Command::IxData { data, program } => {
                use gmsol::decode::{gmsol::store::GMSOLCPIEvent, value::OwnedDataDecoder, Decode};

                let data = data.strip_prefix("0x").unwrap_or(data);
                let data = hex::decode(data).map_err(gmsol::Error::invalid_argument)?;
                let program_id = match program {
                    Program::Store => client.store_program_id(),
                };

                let decoder = OwnedDataDecoder::new(program_id, &data);
                let data = GMSOLCPIEvent::decode(decoder)?;
                println!("{data:#?}");
            }
            Command::Glv {
                select:
                    SelectGlv {
                        address,
                        index,
                        token,
                    },
            } => {
                use anchor_spl::{
                    associated_token::get_associated_token_address, token::TokenAccount,
                };
                use gmsol::types::glv::GlvMarketFlag;

                let address = match (address, token, index) {
                    (Some(address), _, _) => *address,
                    (None, Some(token), _) => client.find_glv_address(token),
                    (None, None, Some(index)) => {
                        let glv_token = client.find_glv_token_address(store, *index);
                        client.find_glv_address(&glv_token)
                    }
                    (None, None, None) => {
                        let glvs = client.glvs(store).await?;
                        let mut table = Table::new();
                        table.set_titles(row![
                            "GLV token",
                            "Index",
                            "Long Token",
                            "Short Token",
                            "Markets"
                        ]);
                        table.set_format(table_format());
                        for (_, glv) in glvs {
                            let glv_token = glv.glv_token();
                            let index = glv.index();
                            let is_valid =
                                client.find_glv_token_address(store, index) == *glv_token;
                            if !is_valid {
                                continue;
                            }
                            let long_token = glv.long_token();
                            let short_token = glv.short_token();
                            let num_markets = glv.num_markets();
                            table.add_row(row![
                                glv_token,
                                index,
                                truncate_pubkey(long_token),
                                truncate_pubkey(short_token),
                                num_markets
                            ]);
                        }

                        println!("{table}");

                        return Ok(());
                    }
                };
                let glv = client
                    .account::<ZeroCopy<types::Glv>>(&address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0;
                println!("Address: {}", address);
                println!("Token: {}", glv.glv_token());
                println!(
                    "Shift last executed: {}",
                    format_ts(
                        time::OffsetDateTime::now_utc(),
                        glv.shift_last_executed_at()
                    )?
                );
                println!("Shift min interval: {}s", glv.shift_min_interval_secs());
                println!(
                    "Shift max price impact: {}%",
                    unsigned_value_to_decimal(glv.shift_max_price_impact_factor()).normalize()
                        * rust_decimal::Decimal::ONE_HUNDRED
                );
                println!(
                    "Shift min value: {}",
                    unsigned_value_to_decimal(glv.shift_min_value()).normalize()
                );
                println!(
                    "Min tokens for first deposit: {}",
                    unsigned_amount_to_decimal(
                        glv.min_tokens_for_first_deposit(),
                        constants::MARKET_TOKEN_DECIMALS
                    )
                    .normalize(),
                );
                println!();
                let mut table = Table::new();
                table.set_titles(row![
                    "Market Token",
                    "Amount",
                    "Vault Balance",
                    "Max Amount",
                    "Max Value",
                    "Allow Deposit"
                ]);
                table.set_format(table_format());
                for market_token in glv.market_tokens() {
                    let config = glv.market_config(&market_token).unwrap();
                    let amount = config.balance();
                    let amount =
                        unsigned_amount_to_decimal(amount, constants::MARKET_TOKEN_DECIMALS)
                            .normalize();
                    let vault = get_associated_token_address(&address, &market_token);
                    let balance = client
                        .account::<TokenAccount>(&vault)
                        .await?
                        .map(|a| a.amount)
                        .unwrap_or(0);
                    let balance =
                        unsigned_amount_to_decimal(balance, constants::MARKET_TOKEN_DECIMALS)
                            .normalize();

                    let max_amount = unsigned_amount_to_decimal(
                        config.max_amount(),
                        constants::MARKET_TOKEN_DECIMALS,
                    )
                    .normalize();
                    let max_value = unsigned_value_to_decimal(config.max_value()).normalize();
                    let is_deposit_allowed = config.get_flag(GlvMarketFlag::IsDepositAllowed);

                    table.add_row(row![
                        market_token,
                        amount,
                        balance,
                        max_amount,
                        max_value,
                        is_deposit_allowed
                    ]);
                }
                println!("{table}");
            }
            Command::GtExchangeVault { address } => {
                let address = match address {
                    Some(address) => *address,
                    None => {
                        let time_window = client.store(store).await?.gt().exchange_time_window();
                        let time_window_index = current_time_window_index(time_window)?;
                        client.find_gt_exchange_vault_address(store, time_window_index, time_window)
                    }
                };
                let vault = client
                    .account::<ZeroCopy<types::gt::GtExchangeVault>>(&address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0;
                println!("{vault:?}");
            }
            Command::PriceFeed { address, authority } => {
                if let Some(address) = address {
                    let feed = client
                        .account::<ZeroCopy<PriceFeed>>(address)
                        .await?
                        .ok_or(gmsol::Error::NotFound)?
                        .0;
                    println!("{feed:#?}");
                } else if let Some(authority) = authority {
                    let feeds = client
                        .store_accounts::<ZeroCopy<PriceFeed>>(
                            None,
                            Some(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                                8 + bytemuck::offset_of!(PriceFeed, authority),
                                authority.as_ref().to_owned(),
                            ))),
                        )
                        .await?;
                    for (feed, _) in feeds {
                        print!("{feed} ");
                    }
                    println!()
                } else {
                    unreachable!()
                }
            }
            Command::Treasury => {
                let config = client.find_treasury_config_address(store);
                println!("Treasury Config: {config}");
                let receiver = client.find_treasury_receiver_address(&config);
                println!("Receiver: {receiver}");
                let config = client
                    .account::<ZeroCopy<types::treasury::Config>>(&config)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0;
                match config.treasury_vault_config() {
                    Some(treasury_vault_config) => {
                        println!("Treasury Vault Config: {treasury_vault_config}")
                    }
                    None => println!("Treasury Vault Config is not set"),
                }
            }
            Command::GtBank {
                address,
                vault,
                date,
                debug,
            } => {
                let address = if let Some(address) = address {
                    *address
                } else {
                    let vault = match vault {
                        Some(vault) => *vault,
                        None => date.get(store, client).await?,
                    };
                    let config = client.find_treasury_config_address(store);
                    let config = client
                        .account::<ZeroCopy<types::treasury::Config>>(&config)
                        .await?
                        .ok_or(gmsol::Error::NotFound)?
                        .0;
                    let treasury_vault_config =
                        config.treasury_vault_config().ok_or_else(|| {
                            gmsol::Error::invalid_argument("treasury vault config is not set")
                        })?;
                    client.find_gt_bank_address(treasury_vault_config, &vault)
                };

                let bank = client
                    .account::<ZeroCopy<types::treasury::GtBank>>(&address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0;
                if *debug {
                    println!("{bank:#?}");
                } else {
                    println!("Address: {address}");
                    println!("Treasury Vault Config: {}", bank.treasury_vault_config());
                    println!("GT Exchange Vault: {}", bank.gt_exchange_vault());
                    for (token, balance) in bank.balances() {
                        println!("{token}: {balance}");
                    }
                }
            }
            Command::Tld { addresses, raw } => {
                use anchor_client::solana_sdk::message::{Message, VersionedMessage};

                let config = client.find_timelock_config_address(store);
                let delay = client
                    .account::<ZeroCopy<types::timelock::TimelockConfig>>(&config)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?
                    .0
                    .delay();
                let delay = time::Duration::seconds(delay as i64);

                let mut instructions = Vec::with_capacity(addresses.len());

                for (idx, address) in addresses.iter().enumerate() {
                    let buffer = client
                        .instruction_buffer(address)
                        .await?
                        .ok_or(gmsol::Error::NotFound)?;

                    let status = match buffer.header().approved_at() {
                        Some(approved_at) => {
                            let approved_at =
                                time::OffsetDateTime::from_unix_timestamp(approved_at)
                                    .map_err(gmsol::Error::unknown)?;
                            let executable_at = approved_at.saturating_add(delay);
                            let now = time::OffsetDateTime::now_utc();
                            let delta = executable_at - now;
                            if delta.is_positive() {
                                format!("executable in {delta}")
                            } else {
                                "executable".to_string()
                            }
                        }
                        None => "is not approved".to_string(),
                    };
                    tracing::info!("[{idx}] {address}: {status}");

                    instructions.push(buffer.to_instruction(true)?);
                }

                let message = Message::new(&instructions, Some(&client.payer()));
                println!(
                    "{}",
                    inspect_transaction(
                        &VersionedMessage::Legacy(message),
                        Some(client.cluster()),
                        *raw,
                    )
                );
            }
            #[cfg(feature = "squads")]
            Command::Squads {
                vault_transaction_address,
                raw,
            } => {
                use gmsol::squads::{
                    get_proposal_pda, get_vault_pda, SquadsProposal, SquadsVaultTransaction,
                };
                use solana_sdk::message::VersionedMessage;

                let vault_transaction = client
                    .account::<SquadsVaultTransaction>(vault_transaction_address)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?;

                let multisig = &vault_transaction.multisig;
                let proposal_pubkey = get_proposal_pda(multisig, vault_transaction.index, None).0;

                let proposal = client
                    .account::<SquadsProposal>(&proposal_pubkey)
                    .await?
                    .ok_or(gmsol::Error::NotFound)?;

                let message = vault_transaction.to_message();

                println!("Transaction Index: {}", vault_transaction.index);
                println!("Status: {:?}", proposal.status);
                println!(
                    "Results: approved {} - rejected {}",
                    proposal.approved.len(),
                    proposal.rejected.len()
                );
                println!("Multisig: {multisig}");
                println!(
                    "Vault: {}",
                    get_vault_pda(multisig, vault_transaction.vault_index, None).0
                );
                println!("Proposal: {proposal_pubkey}");
                println!("Creator: {}", vault_transaction.creator);
                println!(
                    "Inspector: {}",
                    inspect_transaction(
                        &VersionedMessage::V0(message),
                        Some(client.cluster()),
                        *raw
                    )
                );
            }
        }
        Ok(())
    }
}

fn parse_feed_ids(feed_ids: &[String]) -> gmsol::Result<Vec<Identifier>> {
    let feed_ids = feed_ids
        .iter()
        .map(|id| {
            let hex = id.strip_prefix("0x").unwrap_or(id);
            Identifier::from_hex(hex).map_err(gmsol::Error::unknown)
        })
        .collect::<gmsol::Result<Vec<_>>>()?;
    Ok(feed_ids)
}

fn format_market(market: &SerializeMarket) -> gmsol::Result<String> {
    use std::fmt::Write;

    let mut buf = String::new();

    let f = &mut buf;

    writeln!(f, "Name: {}", market.name)?;

    writeln!(f, "\nEnabled: {}", market.enabled)?;

    writeln!(f, "\nAddress: {}", market.address)?;

    writeln!(f, "\nStore: {}", market.store)?;

    writeln!(f, "\nMeta:")?;
    let meta = &market.meta;
    writeln!(f, "market_token: {}", meta.market_token)?;
    writeln!(f, "index_token: {}", meta.index_token)?;
    if meta.long_token == meta.short_token {
        writeln!(f, "single_token: {}", meta.long_token)?;
    } else {
        writeln!(f, "long_token: {}", meta.long_token)?;
        writeln!(f, "short_token: {}", meta.short_token)?;
    }

    writeln!(f, "\nState:")?;
    let state = &market.state;
    if market.is_pure {
        writeln!(
            f,
            "token_balance: {}",
            state.long_token_balance.to_formatted_string(&Locale::en)
        )?;
    } else {
        writeln!(
            f,
            "long_token_balance: {}",
            state.long_token_balance.to_formatted_string(&Locale::en)
        )?;
        writeln!(
            f,
            "short_token_balance: {}",
            state.short_token_balance.to_formatted_string(&Locale::en)
        )?;
    }
    writeln!(
        f,
        "funding_factor_per_second: {}",
        state
            .funding_factor_per_second
            .to_formatted_string(&Locale::en)
    )?;
    writeln!(
        f,
        "deposit_count: {}",
        state.deposit_count.to_formatted_string(&Locale::en)
    )?;
    writeln!(
        f,
        "withdrawal_count: {}",
        state.withdrawal_count.to_formatted_string(&Locale::en)
    )?;
    writeln!(
        f,
        "order_count: {}",
        state.order_count.to_formatted_string(&Locale::en)
    )?;
    writeln!(
        f,
        "trade_count: {}",
        state.trade_count.to_formatted_string(&Locale::en)
    )?;
    let msg = match (
        market.is_adl_enabled_for_long,
        market.is_adl_enabled_for_short,
    ) {
        (true, true) => "enabled for both sides",
        (true, false) => "enabled for long",
        (false, true) => "enabled for short",
        (false, false) => "disabled for both sides",
    };
    writeln!(f, "adl state: {msg}")?;

    writeln!(f, "\nClocks:")?;
    let now = time::OffsetDateTime::now_utc();
    for kind in ClockKind::iter() {
        if let Some(clock) = market.clocks.0.get(&kind) {
            writeln!(f, "{kind}: {}", format_ts(now, *clock)?)?;
        } else {
            writeln!(f, "{kind}: not enabled")?;
        }
    }

    writeln!(f, "\nPools (LONG - SHORT):")?;
    for kind in PoolKind::iter() {
        if let Some(pool) = market.pools.0.get(&kind) {
            writeln!(
                f,
                "{kind}: {} - {}",
                pool.long_amount()?.to_formatted_string(&Locale::en),
                pool.short_amount()?.to_formatted_string(&Locale::en),
            )?;
        } else {
            writeln!(f, "{kind}: not enabled")?;
        }
    }

    writeln!(f, "\nParameters:")?;
    for (key, value) in market.params.0.iter() {
        writeln!(f, "{key} = {}", value.0.to_formatted_string(&Locale::en))?;
    }

    writeln!(f, "\nFlags:")?;
    for (key, value) in market.flags.0.iter() {
        writeln!(f, "{key} = {}", *value)?;
    }

    Ok(buf)
}

fn format_token_config(config: &ser::SerializeTokenConfig) -> gmsol::Result<String> {
    use std::fmt::Write;

    let mut buf = String::new();

    let f = &mut buf;

    writeln!(f, "Name: {}", config.name)?;
    writeln!(f, "Enabled: {}", config.enabled)?;
    writeln!(f, "Synthetic: {}", config.synthetic)?;
    writeln!(f, "Decimals: {}", config.token_decimals)?;
    writeln!(f, "Precision: {}", config.price_precision)?;
    writeln!(f, "Heartbeat: {}", config.heartbeat_duration)?;
    writeln!(f, "Expected Provider: {}", config.expected_provider)?;

    writeln!(f, "\nFeeds:")?;
    for (kind, feed) in config.feeds.iter() {
        let expected = if config.expected_provider == *kind {
            "*"
        } else {
            ""
        };
        writeln!(
            f,
            "{kind}{expected} = {{ feed = {}, timestamp_adjustment = {} }}",
            feed.formatted_feed_id(),
            feed.timestamp_adjustment,
        )?;
    }

    Ok(buf)
}

fn truncate_pubkey(pubkey: &Pubkey) -> String {
    let s = pubkey.to_string();
    let len = s.len();

    if len <= 10 {
        return s.to_string();
    }

    let start = &s[0..6];
    let end = &s[len - 4..];

    format!("{}...{}", start, end)
}

fn format_ts(now: time::OffsetDateTime, clock: i64) -> gmsol::Result<String> {
    let ts = time::OffsetDateTime::from_unix_timestamp(clock).map_err(gmsol::Error::unknown)?;
    let msg = if now >= ts {
        let dur = now - ts;
        format!(
            " ({} ago)",
            humantime::format_duration(dur.try_into().map_err(gmsol::Error::unknown)?)
        )
    } else {
        String::new()
    };

    Ok(format!("{}{msg}", humantime::format_rfc3339(ts.into())))
}
