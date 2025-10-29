use std::ops::Deref;

use anchor_spl::associated_token::get_associated_token_address;
use gmsol_sdk::{
    core::{oracle::PriceProviderKind, token_config::TokenMapAccess},
    ops::AddressLookupTableOps,
    programs::anchor_lang,
    solana_utils::solana_sdk::{pubkey::Pubkey, signer::Signer},
};

/// Address Lookup Table commands.
#[derive(Debug, clap::Args)]
pub struct Alt {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Extend an ALT.
    Extend {
        /// Which kind of addresses to extend.
        #[arg(requires = "alt_input")]
        kind: AltKind,
        /// Whether to init a new ALT.
        #[arg(long, group = "alt_input")]
        init: bool,
        /// The address of the ALT to extend.
        #[arg(long, group = "alt_input")]
        address: Option<Pubkey>,
        /// The authority of the price feed.
        #[arg(long, required_if_eq("kind", "price-feed"))]
        price_feed_authority: Option<Pubkey>,
        /// The index of the price feed.
        #[arg(long, required_if_eq("kind", "price-feed"))]
        price_feed_index: Option<u16>,
        /// The provider kind of the price feed.
        #[arg(long, required_if_eq("kind", "price-feed"))]
        price_feed_provider: Option<PriceProviderKind>,
        /// Custom addresses to extend.
        custom_addresses: Vec<Pubkey>,
    },
}

#[derive(Debug, clap::ValueEnum, Clone)]
enum AltKind {
    /// Custom.
    Custom,
    /// Include common addresses.
    Common,
    /// Include market related addresses.
    Market,
    /// Price Feed.
    PriceFeed,
}

impl super::Command for Alt {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::Extend {
                kind,
                init,
                address,
                price_feed_authority,
                price_feed_index,
                price_feed_provider,
                custom_addresses,
            } => {
                const MAX_ADDRESSES: usize = 256;

                let mut bundle = client.bundle_with_options(options);
                let mut new_addresses = match kind {
                    AltKind::Custom => {
                        vec![]
                    }
                    AltKind::Common => common_addresses(client, store).await?,
                    AltKind::Market => {
                        let mut market_addresses = market_addresses(client, store).await?;
                        let mut glv_addresses = glv_addresses(client, store).await?;
                        market_addresses.append(&mut glv_addresses);
                        market_addresses
                    }
                    AltKind::PriceFeed => {
                        price_feed_addresses(
                            client,
                            store,
                            price_feed_authority.expect("must be provided"),
                            price_feed_index.expect("must be provided"),
                            price_feed_provider.expect("must be provided"),
                        )
                        .await?
                    }
                };

                new_addresses.append(&mut custom_addresses.clone());

                if *init {
                    for chunk in new_addresses.chunks(MAX_ADDRESSES) {
                        let (init, alt) = client.create_alt().await?;
                        bundle.push(init)?;
                        tracing::info!(
                            %alt,
                            "extending ALT with {} addresses: {new_addresses:#?}",
                            new_addresses.len()
                        );
                        println!("{alt}");
                        let extend_txns = client.extend_alt(&alt, chunk.to_vec(), None)?;
                        bundle.append(extend_txns, false)?;
                    }
                } else {
                    let alt = address.expect("must provided");
                    if !new_addresses.is_empty() {
                        tracing::info!(
                            "extending ALT with {} addresses: {new_addresses:#?}",
                            new_addresses.len()
                        );
                        let extend_txns = client.extend_alt(&alt, new_addresses.clone(), None)?;
                        bundle.append(extend_txns, false)?;
                    }

                    println!("{alt}");
                }

                bundle
            }
        };

        client.send_or_serialize(bundle).await?;

        Ok(())
    }
}

async fn common_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    store: &Pubkey,
) -> gmsol_sdk::Result<Vec<Pubkey>> {
    let mut addresses = vec![
        *store,
        client.find_store_wallet_address(store),
        client.store_event_authority(),
        anchor_spl::token::ID,
        anchor_spl::token_2022::ID,
        anchor_spl::associated_token::ID,
        anchor_lang::system_program::ID,
    ];

    if let Some(token_map) = client.authorized_token_map_address(store).await? {
        addresses.push(token_map);
        let token_map = client.token_map(&token_map).await?;
        for token in token_map.tokens() {
            let Some(config) = token_map.get(&token) else {
                continue;
            };
            if !config.is_synthetic() {
                addresses.push(token);
                addresses.push(client.find_market_vault_address(store, &token));
            }
        }
    }

    Ok(addresses)
}

async fn market_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    store: &Pubkey,
) -> gmsol_sdk::Result<Vec<Pubkey>> {
    let mut addresses = Vec::default();

    let markets = client.markets(store).await?;
    for (address, market) in markets {
        addresses.push(address);
        let market_token = market.meta.market_token_mint;
        addresses.push(market_token);
        addresses.push(client.find_market_vault_address(store, &market_token));
    }

    Ok(addresses)
}

async fn glv_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    store: &Pubkey,
) -> gmsol_sdk::Result<Vec<Pubkey>> {
    let mut addresses = Vec::default();

    let glvs = client.glvs(store).await?;
    for (address, glv) in glvs {
        addresses.push(address);
        addresses.push(glv.glv_token);
        for market_token in glv.market_tokens() {
            addresses.push(get_associated_token_address(&address, &market_token));
        }
    }

    Ok(addresses)
}

async fn price_feed_addresses<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    store: &Pubkey,
    authority: Pubkey,
    index: u16,
    provider: PriceProviderKind,
) -> gmsol_sdk::Result<Vec<Pubkey>> {
    let mut addresses = vec![authority];

    if let Some(token_map) = client.authorized_token_map_address(store).await? {
        let token_map = client.token_map(&token_map).await?;
        for token in token_map.tokens() {
            let feed_address =
                client.find_price_feed_address(store, &authority, index, provider, &token);
            addresses.push(feed_address);
        }
    }

    Ok(addresses)
}
