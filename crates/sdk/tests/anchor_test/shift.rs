use gmsol_sdk::{client::ops::ExchangeOps, constants::MARKET_TOKEN_DECIMALS};
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn shift() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("shift");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();

    let long_token_amount = 1_000_008;
    let short_token_amount = 6_000_000_000_007;

    let from_market_token = deployment
        .prepare_market(
            ["fBTC", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let to_market_token = deployment
        .market_token("SOL", "fBTC", "USDG")
        .expect("must exist");

    let unit = 10u64.pow(MARKET_TOKEN_DECIMALS as u32);

    let (rpc, shift) = client
        .create_shift(store, from_market_token, to_market_token, 100 * unit)
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%shift, %signature, "created a shift");

    let signature = client
        .close_shift(&shift)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%shift, %signature, "cancelled a shift");

    let (rpc, shift) = client
        .create_shift(store, from_market_token, to_market_token, 100 * unit)
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%shift, %signature, "created a shift");

    let mut builder = keeper.execute_shift(oracle, &shift, false);
    deployment
        .execute_with_pyth(&mut builder, None, true, true)
        .instrument(tracing::info_span!("execute shift", %shift))
        .await?;

    Ok(())
}
