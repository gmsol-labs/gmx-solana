use gmsol_sdk::client::ops::ExchangeOps;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn basic_swap() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("basic_swap");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let fbtc = deployment.token("fBTC").expect("must exist");
    let usdg = deployment.token("USDG").expect("must exist");

    let long_token_amount = 1_000_011;
    let short_token_amount = 6_000_000_000_013;

    let market_token = deployment
        .prepare_market(
            ["fBTC", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let long_collateral_amount = 100_000;
    let short_collateral_amount = 100 * 100_000_000;
    let times = 8;

    deployment
        .mint_or_transfer_to_user(
            "fBTC",
            Deployment::DEFAULT_USER,
            long_token_amount * times + 17,
        )
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            short_collateral_amount * times + 17,
        )
        .await?;

    for receiver in [keeper.payer(), client.payer()] {
        for side in [true, false] {
            let swap_in_amount = if side {
                long_collateral_amount
            } else {
                short_collateral_amount
            };
            let swap_in_token = if side { &fbtc.address } else { &usdg.address };
            let (rpc, order) = client
                .market_swap(
                    store,
                    market_token,
                    !side,
                    swap_in_token,
                    swap_in_amount,
                    [market_token],
                )
                .receiver(receiver)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %swap_in_amount, %side, %receiver, "created a swap order");

            // Cancel swap.
            let signature = client.close_order(&order)?.build().await?.send().await?;
            tracing::info!(%order, %signature, %swap_in_amount, %side, %receiver, "cancelled the swap order");

            let (rpc, order) = client
                .market_swap(
                    store,
                    market_token,
                    !side,
                    swap_in_token,
                    swap_in_amount,
                    [market_token],
                )
                .receiver(receiver)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %swap_in_amount, %side, %receiver, "created a swap order");

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
                .await?;
        }
    }

    Ok(())
}
