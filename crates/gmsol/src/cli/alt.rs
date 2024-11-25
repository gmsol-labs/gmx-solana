use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{alt::AddressLookupTableOps, types::TokenMapAccess};

use crate::GMSOLClient;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
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
        /// Custom addresses to extend.
        custom_addresses: Vec<Pubkey>,
    },
}

#[derive(clap::ValueEnum, Clone)]
enum AltKind {
    /// Custom.
    Custom,
    /// Include common addresses.
    Common,
    /// Include market related addresses.
    Market,
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::Extend {
                kind,
                init,
                address,
                custom_addresses,
            } => {
                let mut txns = client.transaction();

                let mut new_addresses = match kind {
                    AltKind::Custom => {
                        vec![]
                    }
                    AltKind::Common => common_addresses(client, store).await?,
                    AltKind::Market => market_addresses(client, store).await?,
                };

                new_addresses.append(&mut custom_addresses.clone());

                let alt;
                if *init {
                    let (init, address) = client.create_alt().await?;
                    txns.push(init)?;
                    alt = address;
                } else {
                    alt = address.expect("must provided");
                }

                if !new_addresses.is_empty() {
                    let extend_txns = client.extend_alt(&alt, new_addresses.clone(), None)?;
                    txns.append(extend_txns, false)?;
                }

                if !txns.is_emtpy() {
                    crate::utils::send_or_serialize_transactions(
                        txns,
                        serialize_only,
                        true,
                        |signatures, err| {
                            if let Some(err) = err {
                                tracing::error!(%err, "some txns are failed");
                            }
                            tracing::info!(?new_addresses, "successful txns: {signatures:#?}");
                            println!("{alt}");
                            Ok(())
                        },
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }
}

async fn common_addresses(client: &GMSOLClient, store: &Pubkey) -> gmsol::Result<Vec<Pubkey>> {
    let mut addresses = vec![
        *store,
        client.store_event_authority(),
        anchor_spl::token::ID,
        anchor_spl::token_2022::ID,
        anchor_spl::associated_token::ID,
        anchor_client::anchor_lang::system_program::ID,
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

async fn market_addresses(client: &GMSOLClient, store: &Pubkey) -> gmsol::Result<Vec<Pubkey>> {
    let mut addresses = Vec::default();

    let markets = client.markets(store).await?;
    for (address, market) in markets {
        addresses.push(address);
        let market_token = market.meta().market_token_mint;
        addresses.push(market_token);
        addresses.push(client.find_market_vault_address(store, &market_token));
    }

    Ok(addresses)
}
