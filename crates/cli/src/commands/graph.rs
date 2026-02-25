use std::{collections::BTreeMap, sync::Arc};

use gmsol_sdk::{
    constants::MARKET_TOKEN_DECIMALS,
    core::market::MarketFlag,
    glv::{GlvCalculator, GlvModel},
    market_graph::MarketGraph,
    model::{price::Price, LiquidityMarketExt, MarketModel, PnlFactorKind},
    serde::StringPubkey,
    utils::{zero_copy::try_deserialize_zero_copy_from_base64_with_options, Value},
};
use url::Url;

use crate::{commands::Context, config::DisplayOptions};

const GLV_UNIT: u64 = 10u64.pow(MARKET_TOKEN_DECIMALS as u32);

/// Market Graph Commands.
#[derive(Debug, clap::Args)]
pub struct Graph {
    #[arg(long)]
    api: Url,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Get GLV Token prices.
    GlvTokenPrices {},
    /// Get Market Token Prices.
    MarketTokenPrices {},
}

impl super::Command for Graph {
    fn is_client_required(&self) -> bool {
        matches!(self.command, Command::GlvTokenPrices {})
    }

    async fn execute(&self, ctx: Context<'_>) -> eyre::Result<()> {
        let graph_client = GraphClient::new(&self.api)?;
        let graph = graph_client.create_graph().await?;
        let output = ctx.config().output();
        let store = ctx.store();

        match &self.command {
            Command::GlvTokenPrices {} => {
                let client = ctx.client()?;
                let mut simulator = graph.to_simulator(Default::default());
                let glvs = client.glvs(store).await?;

                for glv in glvs.into_values() {
                    if client.find_glv_token_address(store, glv.index) != glv.glv_token {
                        continue;
                    }
                    if glv.num_markets() == 0 {
                        continue;
                    }
                    let Some(mint) = client
                        .account::<anchor_spl::token_interface::Mint>(&glv.glv_token)
                        .await?
                    else {
                        continue;
                    };
                    let model = GlvModel::new(Arc::new(glv), mint.supply);
                    simulator.insert_glv(model);
                }

                let prices = simulator
                    .glvs()
                    .map(|(glv_token, _)| {
                        let price = simulator.get_glv_token_value(glv_token, GLV_UNIT, false)?;
                        Ok((StringPubkey(*glv_token), Value::from_u128(price)))
                    })
                    .collect::<gmsol_sdk::Result<BTreeMap<_, _>>>()?;

                let prices = prices.iter().map(|(pubkey, price)| {
                    serde_json::json!({
                        "glv_token": pubkey,
                        "price": price,
                    })
                });
                let output = output.display_many(prices, DisplayOptions::default())?;
                println!("{output}");
            }
            Command::MarketTokenPrices {} => {
                let simulator = graph.to_simulator(Default::default());
                let prices = simulator
                    .markets()
                    .filter_map(|(market_token, _)| {
                        let model = simulator.get_market(market_token)?;
                        let prices = simulator.get_prices(&model.meta)?;
                        model
                            .flags
                            .get_flag(MarketFlag::Enabled)
                            .then_some((model, prices))
                    })
                    .map(|(model, prices)| {
                        let price = model.market_token_price(
                            &prices,
                            PnlFactorKind::MaxAfterDeposit,
                            false,
                        )?;
                        let price = Value::from_u128(price);
                        Ok((StringPubkey(model.meta.market_token_mint), price))
                    })
                    .collect::<gmsol_sdk::Result<BTreeMap<_, _>>>()?;
                let prices = prices.iter().map(|(pubkey, price)| {
                    serde_json::json!({
                        "market_token": pubkey,
                        "price": price,
                    })
                });
                let output = output.display_many(prices, DisplayOptions::default())?;
                println!("{output}");
            }
        }

        Ok(())
    }
}

struct GraphClient {
    client: gql_client::Client,
}

impl GraphClient {
    fn new(endpoint: &Url) -> gmsol_sdk::Result<Self> {
        let client = gql_client::Client::new(endpoint);

        Ok(Self { client })
    }

    async fn create_graph(&self) -> gmsol_sdk::Result<MarketGraph> {
        let mut graph = MarketGraph::default();
        let data = self
            .client
            .query::<MarketQueryData>(MARKETS_QUERY)
            .await
            .map_err(gmsol_sdk::Error::custom)?
            .ok_or(gmsol_sdk::Error::NotFound)?;
        for query in data.markets {
            let market = try_deserialize_zero_copy_from_base64_with_options(&query.data, false)?;
            let model = MarketModel::from_parts(Arc::new(market.0), query.market_token_mint.supply);
            graph.insert_market(model);
            let meta = &query.meta;
            graph.update_token_price(
                &meta.index_token.pubkey,
                &meta.index_token.price.to_model()?,
            );
            graph.update_token_price(&meta.long_token.pubkey, &meta.long_token.price.to_model()?);
            graph.update_token_price(
                &meta.short_token.pubkey,
                &meta.short_token.price.to_model()?,
            );
        }
        Ok(graph)
    }
}

const MARKETS_QUERY: &str = r#"
{
  markets {
    marketTokenMint {
      supply
    }
    meta {
      indexToken {
        pubkey
        price {
          min
          max
        }
      }
      longToken {
        pubkey
        price {
          min
          max
        }
      }
      shortToken {
        pubkey
        price {
          min
          max
        }
      }
    }
    data
  }
}
"#;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketQueryData {
    markets: Vec<MarketQuery>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketQuery {
    market_token_mint: MarketTokenMintQuery,
    meta: MarketMetaQuery,
    data: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketTokenMintQuery {
    supply: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketMetaQuery {
    index_token: TokenQuery,
    long_token: TokenQuery,
    short_token: TokenQuery,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenQuery {
    pubkey: StringPubkey,
    price: PriceQuery,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PriceQuery {
    min: String,
    max: String,
}

impl PriceQuery {
    fn to_model(&self) -> gmsol_sdk::Result<Price<u128>> {
        self.try_into()
    }
}

impl<'a> TryFrom<&'a PriceQuery> for Price<u128> {
    type Error = gmsol_sdk::Error;

    fn try_from(value: &'a PriceQuery) -> Result<Self, Self::Error> {
        Ok(Self {
            min: value.min.parse().map_err(gmsol_sdk::Error::custom)?,
            max: value.max.parse().map_err(gmsol_sdk::Error::custom)?,
        })
    }
}
