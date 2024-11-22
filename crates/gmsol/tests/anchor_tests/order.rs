use gmsol::{
    constants::MARKET_USD_UNIT, exchange::ExchangeOps, store::market::MarketOps,
    types::MarketConfigKey,
};
use tracing::Instrument;

use crate::anchor_tests::setup::{current_deployment, Deployment};

#[tokio::test]
async fn balanced_market_order() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("balanced_market_order");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let usdg = deployment.token("USDG").expect("must exist");

    let long_token_amount = 1_000_005;
    let short_token_amount = 6_000_000_000_003;

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

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, long_collateral_amount * 4)
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            short_collateral_amount * 4,
        )
        .await?;

    // Increase position.
    let size = 5_000 * 100_000_000_000_000_000_000;

    for side in [true, false] {
        for collateral_side in [true, false] {
            let collateral_amount = if collateral_side {
                long_collateral_amount
            } else {
                short_collateral_amount
            };
            let (rpc, order) = client
                .market_increase(
                    store,
                    market_token,
                    collateral_side,
                    collateral_amount,
                    side,
                    size,
                )
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %size, "created an increase position order");

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

            // Extract collateral.
            let amount = collateral_amount / 2;
            let (rpc, order) = client
                .market_decrease(store, market_token, collateral_side, amount, side, 0)
                .min_output_amount(u128::MAX)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %amount, "created a extract collateral order");

            let mut builder = keeper.execute_order(store, oracle, &order, true)?;
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

            // Decrease position.
            let (rpc, order) = client
                .market_decrease(store, market_token, collateral_side, 0, side, size)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %size, "created a decrease position order");

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

    let side = true;
    let collateral_side = true;
    let collateral_amount = short_collateral_amount;

    // Increase position with swap path.
    let size = 10_000_000_000_000_000_000_000;
    let (rpc, order) = client
        .market_increase(
            store,
            market_token,
            collateral_side,
            collateral_amount,
            side,
            size,
        )
        .initial_collateral_token(&usdg.address, None)
        .swap_path(vec![*market_token])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, "created an increase position order");

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

    // Extract collateral.
    let amount = 1_00;
    let (rpc, order) = client
        .market_decrease(store, market_token, true, amount, side, 0)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an order to extract collateral");

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

    // Extract collateral and swap.
    let amount = 1_00;
    let (rpc, order) = client
        .market_decrease(store, market_token, true, amount, side, 0)
        .final_output_token(&usdg.address)
        .swap_path(vec![*market_token])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an order to extract collateral and swap");

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

    // Fully decrease and swap.
    let (rpc, order) = client
        .market_decrease(store, market_token, true, 0, side, size)
        .final_output_token(&usdg.address)
        .swap_path(vec![*market_token])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, "created an order to fully decrease the position and swap");

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
    Ok(())
}

#[tokio::test]
async fn single_token_market_order() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("single_token_market_order");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let usdg = deployment.token("USDG").expect("must exist");

    let long_token_amount = 1_000_005;
    let short_token_amount = 6_000_000_000_003;

    let for_swap = deployment
        .prepare_market(
            ["fBTC", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let pool_token_amount = 1_000_007;
    let market_token = deployment
        .prepare_market(["SOL", "fBTC", "fBTC"], pool_token_amount, 0, true)
        .await?;

    let collateral_amount = 100_001;
    let initial_collateral_amount = 103 * 100_000_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount * 4)
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            initial_collateral_amount * 4,
        )
        .await?;

    // Increase position.
    let size = 5_000 * 100_000_000_000_000_000_000;

    for side in [true, false] {
        for collateral_side in [true, false] {
            let (rpc, order) = client
                .market_increase(
                    store,
                    market_token,
                    collateral_side,
                    collateral_amount,
                    side,
                    size,
                )
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %size, "created an increase position order");

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

            // Extract collateral.
            let amount = collateral_amount / 2;
            let (rpc, order) = client
                .market_decrease(store, market_token, collateral_side, amount, side, 0)
                .min_output_amount(u128::MAX)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %amount, "created a extract collateral order");

            let mut builder = keeper.execute_order(store, oracle, &order, true)?;
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

            // Decrease position.
            let (rpc, order) = client
                .market_decrease(store, market_token, collateral_side, 0, side, size)
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %size, "created a decrease position order");

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

    let side = false;
    let collateral_side = false;
    let collateral_amount = initial_collateral_amount;

    // Increase position with swap path.
    let size = 10_000_000_000_000_000_000_000;
    let (rpc, order) = client
        .market_increase(
            store,
            market_token,
            collateral_side,
            collateral_amount,
            side,
            size,
        )
        .initial_collateral_token(&usdg.address, None)
        .swap_path(vec![*for_swap])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, "created an increase position order");

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

    // Extract collateral.
    let amount = 1_00;
    let (rpc, order) = client
        .market_decrease(store, market_token, true, amount, side, 0)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an order to extract collateral");

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

    // Extract collateral and swap.
    let amount = 1_00;
    let (rpc, order) = client
        .market_decrease(store, market_token, true, amount, side, 0)
        .final_output_token(&usdg.address)
        .swap_path(vec![*for_swap])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an order to extract collateral and swap");

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

    // Fully decrease and swap.
    let (rpc, order) = client
        .market_decrease(store, market_token, true, 0, side, size)
        .final_output_token(&usdg.address)
        .swap_path(vec![*for_swap])
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %size, "created an order to fully decrease the position and swap");

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
    Ok(())
}

#[tokio::test]
async fn liquidation() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("liquidation");
    let _enter = span.enter();

    let long_token_amount = 123000 * 100_000_000;
    let short_token_amount = 15 * 1_000_000 / 10;
    let market_token = deployment
        .prepare_market(
            Deployment::SELECT_LIQUIDATION_MARKET,
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();

    {
        let client = deployment.locked_user_client().await?;
        let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

        let usd = 125u64;
        let collateral_amount = usd * 100_000_000;
        let leverage = 50;
        let size = leverage * usd as u128 * MARKET_USD_UNIT;

        deployment
            .mint_or_transfer_to("USDG", &client.payer(), collateral_amount * 3)
            .await?;

        // Open position.
        let (rpc, order, position) = client
            .market_increase(store, market_token, true, collateral_amount, false, size)
            .build_with_addresses()
            .await?;
        let position = position.expect("must have position");
        let signature = rpc.send().await?;
        tracing::info!(%order, %signature, %size, "created an order to increase position");

        let mut builder = keeper.execute_order(store, oracle, &order, false)?;
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .instrument(tracing::info_span!("execute", order=%order))
            .await?;

        let signature = keeper
            .update_market_config_by_key(
                store,
                market_token,
                MarketConfigKey::MinCollateralFactor,
                &MARKET_USD_UNIT,
            )?
            .send()
            .await?;
        tracing::info!(%signature, %market_token, "increased min collateral factor");

        // Liquidate.
        let mut builder = keeper.liquidate(oracle, &position)?;

        deployment
            .execute_with_pyth(
                builder
                    .add_alt(deployment.common_alt().clone())
                    .add_alt(deployment.market_alt().clone()),
                None,
                true,
                true,
            )
            .instrument(tracing::info_span!("liquidate", position=%position))
            .await?;

        let signature = keeper
            .update_market_config_by_key(
                store,
                market_token,
                MarketConfigKey::MinCollateralFactor,
                &(MARKET_USD_UNIT / 100),
            )?
            .send()
            .await?;
        tracing::info!(%signature, %market_token, "restore min collateral factor");
    }

    Ok(())
}
