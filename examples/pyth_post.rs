//! Fetch one upgraded Hermes price and post it to the new Pyth programs.
//!
//! ```text
//! PYTH_API_KEY=... cargo run -p gmsol-examples --example pyth-post
//! ```
//!
//! Optional: `CLUSTER=devnet` (default), `SOLANA_KEYPAIR=~/.config/solana/id.json`,
//! `PYTH_HERMES_URL=https://pyth.dourolabs.app/hermes`.
//! The wallet needs SOL on that cluster. Share the printed signatures with the team.

use std::env;

use gmsol_sdk::{
    client::{
        pull_oracle::PostPullOraclePrices,
        pyth::{
            pull_oracle::{hermes::Identifier, PriceUpdates, PythPullOracleWithHermes},
            EncodingType, Hermes, PythPullOracle,
        },
    },
    solana_utils::solana_sdk::signature::read_keypair_file,
    Client,
};

const UPGRADED_HERMES: &str = "https://pyth.dourolabs.app/hermes";
/// BTC/USD
const FEED_HEX: &str = "e62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43";

#[tokio::main]
async fn main() -> gmsol_sdk::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("pyth_post=info".parse().map_err(gmsol_sdk::Error::custom)?),
        )
        .init();

    let api_key = env::var("PYTH_API_KEY")
        .map_err(|_| gmsol_sdk::Error::custom("set PYTH_API_KEY (do not commit the key)"))?;
    let hermes_url = env::var("PYTH_HERMES_URL").unwrap_or_else(|_| UPGRADED_HERMES.to_string());
    let cluster = env::var("CLUSTER")
        .unwrap_or_else(|_| "devnet".to_string())
        .parse()?;
    let keypair_path = env::var("SOLANA_KEYPAIR").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{home}/.config/solana/id.json")
    });

    let payer = read_keypair_file(&keypair_path).map_err(gmsol_sdk::Error::custom)?;
    let client = Client::new(cluster, &payer)?;
    let pyth = PythPullOracle::try_new(&client)?;
    let hermes = Hermes::try_new_with_api_key(hermes_url, api_key)?;
    let oracle = PythPullOracleWithHermes::from_parts(&client, &hermes, &pyth);

    let feed = Identifier::from_hex(FEED_HEX).map_err(gmsol_sdk::Error::custom)?;
    let update = hermes
        .latest_price_updates([&feed], Some(EncodingType::Base64))
        .await?;
    let parsed = update
        .parsed()
        .first()
        .ok_or_else(|| gmsol_sdk::Error::custom("empty Hermes parsed update"))?;
    println!(
        "fetched {} @ {}",
        parsed.id(),
        parsed.price().publish_time()
    );

    let price_updates = PriceUpdates::from(vec![update.binary().clone()]);
    let (ixns, feeds) = oracle
        .fetch_price_update_instructions(&price_updates, Default::default())
        .await?;
    println!("price accounts: {feeds:?}");

    let (post, _close) = ixns.split();
    let bundle = post.build()?;
    match bundle.send_all(false).await {
        Ok(signatures) => {
            for sig in signatures {
                println!("ok {sig}");
                println!("https://explorer.solana.com/tx/{sig}?cluster=devnet");
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
