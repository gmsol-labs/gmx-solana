use gmsol_sdk::{client::ops::ExchangeOps, constants::MARKET_USD_UNIT};
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn unwrap_native_token_with_swap_path() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("unwrap_native_token_with_swap_path");
    let _enter = span.enter();

    let long_token_amount = 1120 * 1_000_000 / 10_000;
    let long_swap_token_amount = 1130 * 1_000_000_000 / 100;
    let short_token_amount = 2340 * 100_000_000;
    let market_token = deployment
        .prepare_market(
            ["SOL", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;
    let swap_market_token = deployment
        .prepare_market(
            ["fBTC", "WSOL", "USDG"],
            long_swap_token_amount,
            short_token_amount,
            true,
        )
        .await?;
    let wsol = deployment.token("WSOL").unwrap();

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let collateral_amount = 210 * 100_000_000;
    let size = 40 * MARKET_USD_UNIT;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, collateral_amount)
        .await?;

    // Open position.
    let (rpc, order) = client
        .market_increase(store, market_token, false, collateral_amount, true, size)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, "created an order to increase position");

    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .instrument(tracing::info_span!("execute", order=%order))
        .await?;

    // Close position.
    let receiver = keeper.payer();
    let (rpc, order) = client
        .market_decrease(store, market_token, false, collateral_amount, true, size)
        .final_output_token(&wsol.address)
        .swap_path(vec![*swap_market_token])
        .receiver(receiver)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, %receiver, "created an order to close position");

    let mut builder = keeper.execute_order(store, oracle, &order, false)?;
    deployment
        .execute_with_pyth(
            builder
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .instrument(tracing::info_span!("execute", order=%order))
        .await?;

    Ok(())
}
