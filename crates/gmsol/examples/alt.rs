use std::sync::Arc;

use anchor_client::solana_sdk::{
    signature::Keypair,
    signer::{EncodableKey, Signer},
};
use eyre::eyre;
use gmsol::alt::AddressLookupTableOps;
use gmsol_solana_utils::cluster::Cluster;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let path = std::env::var("WALLET").unwrap_or_else(|_| "~/.config/solana/id.json".to_string());
    let wallet = shellexpand::full(&path)?;
    let wallet = Keypair::read_from_file(wallet.as_ref())
        .map_err(|err| eyre!("Failed to load keypair: {err}"))?;
    let client = gmsol::Client::new(Cluster::Devnet, Arc::new(wallet))?;

    let (rpc, alt) = client.create_alt().await?;
    let signature = rpc.send().await?;
    tracing::info!(%alt, %signature, "ALT created");

    let random_address = Keypair::new();
    let signatures = client
        .extend_alt(&alt, vec![random_address.pubkey()], None)?
        .send_all(false)
        .await
        .map_err(|(_, err)| err)?;
    tracing::info!(%alt, ?signatures, "ALT extended, new address={}", random_address.pubkey());

    let account = client.alt(&alt).await?;
    tracing::info!(%alt, "ALT fetched {account:?}");

    let signature = client.deactivate_alt(&alt).send().await?;
    tracing::info!(%alt, %signature, "ALT deactivated");

    // ALT can only be closed after it has been fully deactivated (after 512 blocks).
    // let signature = client.close_alt(&alt).send().await?;
    // tracing::info!(%alt, %signature, "ALT closed");

    Ok(())
}
