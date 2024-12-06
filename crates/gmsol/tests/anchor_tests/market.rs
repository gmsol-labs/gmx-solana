use gmsol::{
    exchange::ExchangeOps, store::market::MarketOps, types::market::config::MarketConfigFlag,
};

use crate::anchor_tests::setup::{current_deployment, Deployment};

#[tokio::test]
async fn claim_fees() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("claim_fees");
    let _enter = span.enter();

    let store = &deployment.store;
    let market_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");

    deployment.wait_until_claim_fees_enabled().await;

    let signature = deployment
        .client
        .claim_fees(store, market_token, false)
        .build()
        .await?
        .send_without_preflight()
        .await?;

    tracing::info!(%signature, "claimed fees");

    Ok(())
}

#[tokio::test]
async fn set_market_config_flag() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("set_market_config_flag");
    let _enter = span.enter();

    let store = &deployment.store;
    let market_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");

    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let signature = client
        .update_market_config_flag_by_key(
            store,
            market_token,
            MarketConfigFlag::SkipBorrowingFeeForSmallerSide,
            true,
        )?
        .send_without_preflight()
        .await?;

    tracing::info!(%signature, "update market config flag");

    Ok(())
}
