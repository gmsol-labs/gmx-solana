use gmsol_sdk::client::ops::MarketOps;
use gmsol_utils::market::MarketConfigFlag;
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

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
            false,
        )?
        .send_without_preflight()
        .await?;

    tracing::info!(%signature, "update market config flag");

    Ok(())
}

#[tokio::test]
async fn get_market_token_value() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("get_market_token_value");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();

    let long_token_amount = 1_000_011;
    let short_token_amount = 6_000_000_000_009;

    let market_token_amount = 1_234_567_890;

    let market_token = deployment
        .prepare_market(
            ["SOL", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let mut builder =
        keeper.get_market_token_value(store, oracle, market_token, market_token_amount);
    deployment
        .execute_with_pyth(&mut builder, None, true, true)
        .instrument(
            tracing::info_span!("get market token value", %market_token, %market_token_amount),
        )
        .await?;

    Ok(())
}
