//! Smoke check for the Pyth post path: fetch one price from Hermes with an
//! API key and post it on-chain through the SDK's pull-oracle helpers.
//!
//! ```text
//! PYTH_API_KEY=... cargo run -p gmsol-examples --example pyth-post
//! ```
//!
//! Optional env:
//! - `CLUSTER` (default `devnet`)
//! - `SOLANA_KEYPAIR` (default `~/.config/solana/id.json`); the wallet needs SOL
//! - `PYTH_HERMES_URL` (default: the SDK's `DEFAULT_HERMES_BASE`); a path
//!   prefix such as `https://pyth.dourolabs.app/hermes` is preserved
//! - `PYTH_FEED_ID` hex feed id (default BTC/USD)
//!
//! Prints the fetched publish time and the transaction signatures.

use std::env;

use gmsol_sdk::{
    client::{
        pull_oracle::PostPullOraclePrices,
        pyth::{
            pull_oracle::{
                hermes::{Identifier, DEFAULT_HERMES_BASE, ENV_API_KEY},
                PriceUpdates, PythPullOracleWithHermes,
            },
            EncodingType, Hermes, PythPullOracle,
        },
    },
    solana_utils::solana_sdk::signature::read_keypair_file,
    Client,
};

/// BTC/USD
const DEFAULT_FEED_HEX: &str = "e62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43";

#[tokio::main]
async fn main() -> gmsol_sdk::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("pyth_post=info".parse().map_err(gmsol_sdk::Error::custom)?),
        )
        .init();

    let api_key = env::var(ENV_API_KEY)
        .map_err(|_| gmsol_sdk::Error::custom(format!("set {ENV_API_KEY}")))?;
    let hermes_url =
        env::var("PYTH_HERMES_URL").unwrap_or_else(|_| DEFAULT_HERMES_BASE.to_string());
    let cluster = env::var("CLUSTER")
        .unwrap_or_else(|_| "devnet".to_string())
        .parse()?;
    let keypair_path = env::var("SOLANA_KEYPAIR")
        .map(|path| shellexpand::tilde(&path).into_owned())
        .unwrap_or_else(|_| shellexpand::tilde("~/.config/solana/id.json").into_owned());
    let feed_hex = env::var("PYTH_FEED_ID").unwrap_or_else(|_| DEFAULT_FEED_HEX.to_string());

    let payer = read_keypair_file(&keypair_path).map_err(gmsol_sdk::Error::custom)?;
    let client = Client::new(cluster, &payer)?;
    let pyth = PythPullOracle::try_new(&client)?;
    let hermes = Hermes::try_new_with_api_key(&hermes_url, api_key)?;
    let oracle = PythPullOracleWithHermes::from_parts(&client, &hermes, &pyth);

    let feed = Identifier::from_hex(&feed_hex).map_err(gmsol_sdk::Error::custom)?;
    let update = hermes
        .latest_price_updates([&feed], Some(EncodingType::Base64))
        .await?;
    let parsed = update
        .parsed()
        .first()
        .ok_or_else(|| gmsol_sdk::Error::custom("empty Hermes parsed update"))?;
    println!(
        "fetched feed {} publish_time={} via {hermes_url}",
        parsed.id(),
        parsed.price().publish_time()
    );

    let price_updates = PriceUpdates::from(vec![update.binary().clone()]);
    let (ixns, feeds) = oracle
        .fetch_price_update_instructions(&price_updates, Default::default())
        .await?;
    println!("price update accounts: {feeds:?}");

    let (post, _close) = ixns.split();
    let bundle = post.build()?;
    match bundle.send_all(false).await {
        Ok(signatures) => {
            for sig in signatures {
                println!("ok {sig}");
            }
            Ok(())
        }
        Err((signatures, err)) => {
            for sig in &signatures {
                println!("partial {sig}");
            }
            Err(gmsol_sdk::Error::custom(err.to_string()))
        }
    }
}
