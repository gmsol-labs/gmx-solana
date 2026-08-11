use std::time::Duration;

use anchor_spl::associated_token::get_associated_token_address;
use eyre::OptionExt;
use gmsol_programs::gmsol_store::types::{DecreasePositionSwapType, UpdateOrderParams};
use gmsol_sdk::{
    client::ops::{ExchangeOps, MarketOps},
    constants::MARKET_USD_UNIT,
};
use gmsol_store::CoreError;
use gmsol_utils::market::MarketConfigKey;
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

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
    let times = 8;

    deployment
        .mint_or_transfer_to_user(
            "fBTC",
            Deployment::DEFAULT_USER,
            long_collateral_amount * times,
        )
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            short_collateral_amount * times,
        )
        .await?;

    // Increase position.
    let size = 5_000 * 100_000_000_000_000_000_000;

    for receiver in [keeper.payer(), client.payer()] {
        for side in [true, false] {
            for collateral_side in [true, false] {
                let collateral_amount = if collateral_side {
                    long_collateral_amount
                } else {
                    short_collateral_amount
                };
                // Increase position.
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
                tracing::info!(%order, %signature, %size, %receiver, "created an increase position order");

                // Cancel.
                let signature = client.close_order(&order)?.build().await?.send().await?;
                tracing::info!(%order, %signature, %size, %receiver, "increase position order cancelled");

                tokio::time::sleep(Duration::from_secs(2)).await;

                // Increase position again.
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
                tracing::info!(%order, %signature, %size, %receiver, "created an increase position order");

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

                // Increase position again.
                let increment_size = size / 10;
                let (rpc, order) = client
                    .market_increase(
                        store,
                        market_token,
                        collateral_side,
                        0,
                        side,
                        increment_size,
                    )
                    .receiver(receiver)
                    .build_with_address()
                    .await?;
                let signature = rpc.send().await?;
                tracing::info!(%order, %signature, %increment_size, %receiver, "created an increase position order");

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
                    .decrease_position_swap_type(Some(
                        DecreasePositionSwapType::CollateralToPnlToken,
                    ))
                    .min_output_amount(u128::MAX)
                    .receiver(receiver)
                    .build_with_address()
                    .await?;
                let signature = rpc.send().await?;
                tracing::info!(%order, %signature, %amount, %receiver, "created a extract collateral order");

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
                    .market_decrease(
                        store,
                        market_token,
                        collateral_side,
                        0,
                        side,
                        size + increment_size,
                    )
                    .decrease_position_swap_type(Some(
                        DecreasePositionSwapType::PnlTokenToCollateralToken,
                    ))
                    .receiver(receiver)
                    .build_with_address()
                    .await?;
                let signature = rpc.send().await?;
                tracing::info!(%order, %signature, %size, %receiver, "created a decrease position order");

                // Cancel.
                let signature = client.close_order(&order)?.build().await?.send().await?;
                tracing::info!(%order, %signature, %size, %receiver, "decrease position order cancelled");

                // Decrease position again.
                let (rpc, order) = client
                    .market_decrease(
                        store,
                        market_token,
                        collateral_side,
                        0,
                        side,
                        size + increment_size,
                    )
                    .decrease_position_swap_type(Some(
                        DecreasePositionSwapType::PnlTokenToCollateralToken,
                    ))
                    .receiver(receiver)
                    .build_with_address()
                    .await?;
                let signature = rpc.send().await?;
                tracing::info!(%order, %signature, %size, %receiver, "created a decrease position order");

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
    }

    let side = false;
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
        .decrease_position_swap_type(Some(DecreasePositionSwapType::CollateralToPnlToken))
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
        .decrease_position_swap_type(Some(DecreasePositionSwapType::PnlTokenToCollateralToken))
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
    let times = 4;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount * times)
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            initial_collateral_amount * times,
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

            // Increase position
            let increment_size = size / 10;
            let (rpc, order) = client
                .market_increase(
                    store,
                    market_token,
                    collateral_side,
                    0,
                    side,
                    increment_size,
                )
                .build_with_address()
                .await?;
            let signature = rpc.send().await?;
            tracing::info!(%order, %signature, %increment_size, "created an increase position order");

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
                .decrease_position_swap_type(Some(DecreasePositionSwapType::CollateralToPnlToken))
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
                .market_decrease(
                    store,
                    market_token,
                    collateral_side,
                    0,
                    side,
                    size + increment_size,
                )
                .decrease_position_swap_type(Some(
                    DecreasePositionSwapType::PnlTokenToCollateralToken,
                ))
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
        .decrease_position_swap_type(Some(DecreasePositionSwapType::CollateralToPnlToken))
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
        .decrease_position_swap_type(Some(DecreasePositionSwapType::PnlTokenToCollateralToken))
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
                MarketConfigKey::MinCollateralFactorForLiquidation,
                &MARKET_USD_UNIT,
            )?
            .send_without_preflight()
            .await?;
        tracing::info!(%signature, %market_token, "increased min collateral factor");

        let signature = keeper
            .update_market_config_by_key(
                store,
                market_token,
                MarketConfigKey::LiquidationFeeFactor,
                &(5 * MARKET_USD_UNIT / 10_000),
            )?
            .send_without_preflight()
            .await?;
        tracing::info!(%signature, %market_token, "set liquidation fee factor");

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
                MarketConfigKey::MinCollateralFactorForLiquidation,
                &(MARKET_USD_UNIT / 200),
            )?
            .send_without_preflight()
            .await?;
        tracing::info!(%signature, %market_token, "restore min collateral factor");
    }

    Ok(())
}

#[tokio::test]
async fn update_order() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("update_order");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");

    let long_token_amount = 1_000_011;
    let short_token_amount = 6_000_000_000_007;

    let market_token = deployment
        .prepare_market(
            ["fBTC", "fBTC", "USDG"],
            long_token_amount,
            short_token_amount,
            true,
        )
        .await?;

    let long_collateral_amount = 100_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, long_collateral_amount)
        .await?;

    let size = 5_000 * 100_000_000_000_000_000_000;
    let price_1 = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);
    let (rpc, order) = client
        .limit_increase(
            store,
            market_token,
            false,
            size,
            price_1,
            true,
            long_collateral_amount,
        )
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %price_1, %size, "created a limit order");

    let price_2 = price_1 / 2;
    let signature = client
        .update_order(
            store,
            market_token,
            &order,
            UpdateOrderParams {
                trigger_price: Some(price_2),
                ..Default::default()
            },
            None,
        )
        .await?
        .send()
        .await?;

    tracing::info!(%order, %signature, %price_2, %size, "updated a limit order");

    let signature = client.close_order(&order)?.build().await?.send().await?;

    tracing::info!(%order, %signature, "cancelled a limit order");

    Ok(())
}

/// An increase order created **without** the optional final output token escrow keeps behaving as
/// it did before: the final output token stays uninitialized, while the long and short token slots
/// are filled. This is the backward-compatibility case, and the one every client that does not
/// intend to set a builder fee keeps hitting.
#[tokio::test]
async fn increase_order_without_escrow_leaves_final_output_token_uninitialized() -> eyre::Result<()>
{
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("increase_order_without_escrow");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    let collateral_amount = 100_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount)
        .await?;

    let size = 5_000 * MARKET_USD_UNIT;
    let price = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);
    let (rpc, order) = client
        .limit_increase(
            store,
            market_token,
            false,
            size,
            price,
            true,
            collateral_amount,
        )
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created a limit increase order");

    let created = client.order(&order).await?;
    let final_output_token = created.tokens.final_output_token.token_and_account();
    let long_token = created.tokens.long_token.token_and_account();

    tracing::info!(?final_output_token, ?long_token, "order token slots");
    assert!(
        final_output_token.is_none(),
        "an increase order created without the escrow must leave the final output token uninitialized, got {final_output_token:?}"
    );
    assert!(
        long_token.is_some(),
        "the long token slot must be initialized at creation"
    );

    let signature = client.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "cancelled the order");

    Ok(())
}

/// An increase order created **with** the optional final output token escrow records both halves of
/// the final output token: the mint is the position's collateral token, and the account is that
/// token's escrow under the order. This is the shape a builder fee needs, since the fee is charged
/// in the final output token and paid out of that escrow.
#[tokio::test]
async fn increase_order_with_escrow_initializes_final_output_token() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("increase_order_with_escrow");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let fbtc = deployment.token("fBTC").expect("must exist");

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    // Deliberately small: this test shares the market with the other order tests and executes for
    // real, so it keeps its footprint on open interest and pool balances negligible.
    let collateral_amount = 10_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount * 2)
        .await?;

    let size = 100 * MARKET_USD_UNIT;
    let (rpc, order) = client
        .market_increase(store, market_token, true, collateral_amount, true, size)
        .prepare_final_output_token_escrow(true)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an increase order with the escrow");

    let created = client.order(&order).await?;
    let (token, account) = created
        .tokens
        .final_output_token
        .token_and_account()
        .ok_or_eyre("final output token must be initialized when the escrow is provided")?;

    assert_eq!(
        token, fbtc.address,
        "the final output token must be the collateral token"
    );
    assert_eq!(
        account,
        get_associated_token_address(&order, &token),
        "the final output token account must be the order's escrow for that token"
    );

    // Executing it is what exercises the execution-time validation: with the slot initialized, the
    // check that the final output token is the collateral token actually runs, instead of being
    // skipped as it is for orders created without the escrow.
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
    tracing::info!(%order, "executed the increase order carrying the escrow");

    Ok(())
}

/// Creating an increase order whose final output token is not the position's collateral token must
/// revert, leaving no order account behind. The SDK always passes the collateral token for increase
/// orders, so the mismatch is produced by rewriting that one account on the built instruction.
///
/// The order is deliberately built **without** the escrow: when the escrow is provided, Anchor's
/// own `associated_token::mint = final_output_token` constraint rejects a rewritten mint before the
/// creation-time validation is reached, so this path is the only one that exercises it.
#[tokio::test]
async fn increase_order_with_mismatched_final_output_token_should_fail() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("increase_order_with_mismatched_final_output_token");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");
    let usdg = deployment.token("USDG").expect("must exist");

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    let collateral_amount = 100_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount)
        .await?;

    let size = 5_000 * MARKET_USD_UNIT;
    let price = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);
    let (rpc, order) = client
        .limit_increase(
            store,
            market_token,
            false,
            size,
            price,
            true,
            collateral_amount,
        )
        .build_with_address()
        .await?;

    // The collateral is the long token (fBTC), so pointing the final output token at the short
    // token (USDG) is exactly the case the creation-time validation must reject. Only that one
    // account may be rewritten: fBTC is also the initial collateral token and the long token, and
    // it appears again in the escrow-preparation instructions, so a blanket search-and-replace
    // would trip a different check first.
    const FINAL_OUTPUT_TOKEN_INDEX: usize = 8;
    // Without the compute budget instructions: they are re-added when the rewritten instructions
    // are sent, and Solana rejects a transaction that carries the same one twice.
    let mut instructions = rpc.instructions_with_options(true, None, None);
    let create_order = instructions
        .iter_mut()
        .filter(|ix| ix.program_id == gmsol_store::ID)
        .max_by_key(|ix| ix.accounts.len())
        .ok_or_eyre("the create-order instruction must be present")?;
    let account = create_order
        .accounts
        .get_mut(FINAL_OUTPUT_TOKEN_INDEX)
        .ok_or_eyre("the create-order instruction must carry the final output token")?;
    assert_eq!(
        account.pubkey, fbtc.address,
        "account {FINAL_OUTPUT_TOKEN_INDEX} is expected to be the final output token; the account \
         order of `CreateOrderV2` must have changed"
    );
    account.pubkey = usdg.address;

    let err = client
        .store_transaction()
        .pre_instructions(instructions, true)
        .send()
        .await
        .expect_err("the final output token must equal the collateral token");
    let err = gmsol_sdk::Error::from(err);
    assert_eq!(
        err.anchor_error_code(),
        Some(CoreError::TokenMintMismatched.into()),
        "unexpected error: {err:?}"
    );

    let err = client
        .order(&order)
        .await
        .err()
        .ok_or_eyre("no order account may exist after the creation reverted")?;
    assert!(
        matches!(err, gmsol_sdk::Error::NotFound),
        "expected the order account to be absent, got {err:?}"
    );

    Ok(())
}
