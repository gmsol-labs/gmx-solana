use std::collections::BTreeMap;

use either::Either;
use gmsol_sdk::serde::{serde_market::SerdeMarket, StringPubkey};

/// Commands for markets.
#[derive(Debug, clap::Args)]
pub struct Market {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Get market info.
    Get {
        #[arg(long)]
        as_market_addresses: bool,
        addresses: Vec<StringPubkey>,
    },
}

impl super::Command for Market {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();

        match &self.command {
            Command::Get {
                addresses,
                as_market_addresses,
            } => {
                let markets = if addresses.is_empty() {
                    client.markets(store).await?
                } else {
                    let addresses = if *as_market_addresses {
                        Either::Left(addresses.iter().map(|a| a.0))
                    } else {
                        Either::Right(
                            addresses
                                .iter()
                                .map(|a| client.find_market_address(store, a)),
                        )
                    };

                    let mut markets = BTreeMap::default();
                    for address in addresses {
                        let market = client.market(&address).await?;
                        markets.insert(address, market);
                    }

                    markets
                };
                let token_map = client.authorized_token_map(store).await?;
                let serde_markets = markets
                    .iter()
                    .map(|(p, m)| {
                        SerdeMarket::from_market(m, &token_map).map(|m| (p.to_string(), m))
                    })
                    .collect::<gmsol_sdk::Result<BTreeMap<_, _>>>()?;
                println!("{}", serde_json::to_string_pretty(&serde_markets)?);
            }
        }

        Ok(())
    }
}
