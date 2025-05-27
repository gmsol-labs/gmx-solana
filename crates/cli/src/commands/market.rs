use gmsol_sdk::serde::{serde_market::SerdeMarket, StringPubkey};

use crate::config::DisplayOptions;

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
        #[arg(group = "market-address")]
        market_token: Option<StringPubkey>,
        #[arg(long, group = "market-address")]
        address: Option<StringPubkey>,
    },
}

impl super::Command for Market {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let output = ctx.config().output();

        match &self.command {
            Command::Get {
                address,
                market_token,
            } => {
                if address.is_none() && market_token.is_none() {
                    let markets = client.markets(store).await?;
                    let token_map = client.authorized_token_map(store).await?;
                    let mut serde_markets = markets
                        .iter()
                        .map(|(p, m)| SerdeMarket::from_market(m, &token_map).map(|m| (p, m)))
                        .collect::<gmsol_sdk::Result<Vec<(_, _)>>>()?;
                    serde_markets.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
                    serde_markets.sort_by_key(|(_, m)| m.enabled);
                    println!(
                        "{}",
                        output.display_keyed_accounts(
                            serde_markets,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("market_token", "Market Token"),
                                ("enabled", "Is Enabled"),
                                ("long_token_balance", "◎ Long Token"),
                                ("short_token_balance", "◎ Short Token")
                            ]),
                        )?
                    );
                } else {
                    let address = if let Some(address) = address {
                        **address
                    } else if let Some(market_token) = market_token {
                        client.find_market_address(store, market_token)
                    } else {
                        unreachable!()
                    };
                    let market = client.market(&address).await?;
                    let token_map = client.authorized_token_map(store).await?;
                    let market = SerdeMarket::from_market(&market, &token_map)?;
                    println!(
                        "{}",
                        output.display_keyed_account(
                            &address,
                            market,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("pubkey", "Address"),
                                ("market_token", "Market Token"),
                                ("index_token", "Index Token"),
                                ("long_token", "Long Token"),
                                ("short_token", "Short Token"),
                                ("enabled", "Is Enabled"),
                                ("is_pure", "Is Pure"),
                                ("is_adl_enabled_for_long", "Is ADL Enabled (Long)"),
                                ("is_adl_enabled_for_short", "Is ADL Enabled (Short)"),
                                ("is_gt_minting_enabled", "Is GT Minting Enabled"),
                                ("long_token_balance", "◎ Long Token"),
                                ("short_token_balance", "◎ Short Token"),
                                ("funding_factor_per_second", "Funding Factor"),
                            ])
                        )?
                    );
                }
            }
        }

        Ok(())
    }
}
