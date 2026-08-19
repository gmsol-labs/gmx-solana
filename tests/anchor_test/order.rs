use std::time::Duration;

use anchor_spl::associated_token::get_associated_token_address;
use eyre::OptionExt;
use gmsol_programs::{
    anchor_lang::error::ErrorCode,
    gmsol_store::{
        client::{accounts, args},
        types::{DecreasePositionSwapType, UpdateOrderParams},
    },
};
use gmsol_sdk::{
    builders::order::SetBuilderFeeHint,
    client::ops::{BuilderFeeOps, ConfigOps, ExchangeOps, MarketOps, UserOps},
    constants::MARKET_USD_UNIT,
    pda::find_user_token_controller_address,
};
use gmsol_store::CoreError;
use gmsol_utils::{config::FactorKey, market::MarketConfigKey};
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
/// The order is deliberately built **without** the escrow, which is also the case only the
/// instruction-layer check can catch: with no escrow the operation never learns which mint was
/// requested. When the escrow is provided, Anchor's own `associated_token::mint =
/// final_output_token` constraint rejects a rewritten mint before either validation is reached.
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

/// The full checkpoint lifecycle: advertise, checkpoint, re-checkpoint, revoke.
///
/// Every case here needs the store's builder fee cap raised, so the whole body
/// runs inside [`Deployment::with_builder_fee_cap`], which also serializes it
/// against the other tests that move that cap.
#[tokio::test]
async fn set_builder_fee() -> eyre::Result<()> {
    /// One percent, in the market factor unit.
    const CAP: u128 = MARKET_USD_UNIT / 100;

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("set_builder_fee");
    let _enter = span.enter();

    let owner = deployment.user_client(Deployment::DEFAULT_USER)?;
    let builder = deployment.user_client(Deployment::USER_1)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    // Both sides need a User Account: the builder to advertise a factor, the
    // owner because checkpointing its own account is the revocation path.
    for client in [&owner, &builder] {
        client.prepare_user(store)?.send_without_preflight().await?;
    }
    let owner_user = owner.find_user_address(store, &owner.payer());
    let builder_user = builder.find_user_address(store, &builder.payer());

    // The claim vault has to exist before a checkpoint is allowed, and this
    // instruction deliberately creates nothing. Minting zero is how the fixture
    // opens an ATA without moving a balance.
    for user in [&owner_user, &builder_user] {
        deployment.mint_or_transfer_to("fBTC", user, 0).await?;
    }

    let collateral_amount = 100_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount)
        .await?;

    // A limit order priced far from the market stays pending for the whole
    // test, which is the state a checkpoint requires. The escrow is what makes
    // it fee-eligible: without it the final output token is never initialized.
    let size = 5_000 * MARKET_USD_UNIT;
    let price = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);
    let (rpc, order) = owner
        .limit_increase(
            store,
            market_token,
            false,
            size,
            price,
            true,
            collateral_amount,
        )
        .prepare_final_output_token_escrow(true)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created a fee-eligible limit increase order");

    deployment
        .with_builder_fee_cap(CAP, async {
            let signature = builder
                .set_builder_fee_factor(store, CAP)?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "the builder advertised its factor");

            // AC2: the checkpoint carries the factor the owner signed for, and
            // one unit off in either direction is rejected. Exact equality is
            // what stops a builder raising its rate after the owner decided.
            for expected in [CAP - 1, CAP + 1] {
                let err = owner
                    .set_builder_fee(store, &order, &builder_user, expected, None)
                    .await?
                    .send()
                    .await
                    .expect_err("should reject a factor the builder is not advertising");
                assert_eq!(
                    gmsol_sdk::Error::from(err).anchor_error_code(),
                    Some(CoreError::BuilderFeeFactorMismatched.into()),
                );
            }

            // AC1: only the order's owner may checkpoint onto it. Signed here by
            // the builder, which is the party that would gain from it.
            let err = builder
                .set_builder_fee(store, &order, &builder_user, CAP, None)
                .await?
                .send()
                .await
                .expect_err("should reject a checkpoint signed by anyone but the order's owner");
            assert_eq!(
                gmsol_sdk::Error::from(err).anchor_error_code(),
                Some(CoreError::OwnerMismatched.into()),
            );

            // The happy path.
            let signature = owner
                .set_builder_fee(store, &order, &builder_user, CAP, None)
                .await?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "checkpointed the builder fee");

            let checkpointed = owner.order(&order).await?;
            assert_eq!(
                checkpointed.builder, builder_user,
                "the builder must be recorded on the order"
            );
            assert_eq!(
                checkpointed.builder_fee_factor, CAP,
                "the advertised factor must be the one recorded"
            );

            // AC8: updating the order is not a way to change the checkpoint.
            // `update_order_v2` writes none of these fields, and this is the
            // regression guard for that.
            let signature = owner
                .update_order(
                    store,
                    market_token,
                    &order,
                    UpdateOrderParams {
                        trigger_price: Some(price / 2),
                        ..Default::default()
                    },
                    None,
                )
                .await?
                .send()
                .await?;
            tracing::info!(%signature, "updated the order after checkpointing");

            let after_update = owner.order(&order).await?;
            assert_eq!(
                after_update.builder, builder_user,
                "an order update must leave the checkpointed builder untouched"
            );
            assert_eq!(
                after_update.builder_fee_factor, CAP,
                "an order update must leave the checkpointed factor untouched"
            );

            // Revocation. A fresh User Account advertises zero, so checkpointing
            // one is how a builder is dropped; the owner's own account is the
            // natural choice and is deliberately allowed.
            let signature = owner
                .set_builder_fee(store, &order, &owner_user, 0, None)
                .await?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "revoked the builder fee");

            let revoked = owner.order(&order).await?;
            assert_eq!(revoked.builder, owner_user);
            assert_eq!(
                revoked.builder_fee_factor, 0,
                "checkpointing a zero-advertising account must clear the fee"
            );

            // AC3: the cap is enforced again at checkpoint time, not only when
            // the rate is advertised, so a rate that was legal when advertised
            // stops being payable once the cap drops under it.
            let signature = keeper
                .insert_global_factor_by_key(store, FactorKey::MaxBuilderFeeFactor, &(CAP / 2))
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "lowered the cap under the advertised factor");

            let err = owner
                .set_builder_fee(store, &order, &builder_user, CAP, None)
                .await?
                .send()
                .await
                .expect_err("should reject a factor above the lowered cap");
            assert_eq!(
                gmsol_sdk::Error::from(err).anchor_error_code(),
                Some(CoreError::BuilderFeeFactorExceedsMaxFactor.into()),
            );

            let unchanged = owner.order(&order).await?;
            assert_eq!(
                unchanged.builder, owner_user,
                "a rejected checkpoint must leave the previous one in place"
            );

            // The builder's User Account is shared with other tests, so put its
            // advertised factor back where it was found.
            builder
                .set_builder_fee_factor(store, 0)?
                .send_without_preflight()
                .await?;

            Ok::<_, eyre::Report>(())
        })
        .await?;

    let signature = owner.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "cancelled the order");

    Ok(())
}

/// The orders, account shapes and store states a checkpoint must refuse.
///
/// None of these need the fee cap raised: the builder here is the caller's own
/// User Account, which advertises zero, and zero is payable under any cap. That
/// keeps most of this test out of the lock that serializes [`set_builder_fee`].
///
/// The exception is the disabled-feature case at the end, which moves a store
/// global and so takes that lock for its window through
/// [`Deployment::with_builder_fee_disabled`]. The cases above it run unlocked
/// and are unaffected, being the same test and therefore sequential.
///
/// Liquidation and `AutoDeleveraging` are the two kinds AC4 exists for, and they
/// are deliberately absent: `create_order` refuses both (`OrderKindNotAllowed`,
/// `instructions/exchange/order.rs`) and the keeper builds a liquidation inside
/// the transaction that executes it, so no pending order of either kind can be
/// reached from a test. They are covered instead by the unit tests on
/// `OrderKind::is_user_initiated_position` in `crates/utils/src/order.rs`.
#[tokio::test]
async fn set_builder_fee_rejects_ineligible_orders() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("set_builder_fee_rejects_ineligible_orders");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");
    let usdg = deployment.token("USDG").expect("must exist");

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    // The caller is its own builder here. A User Account advertises zero until
    // its owner sets a factor, and nothing else in the suite moves this one, so
    // the expected factor stays zero without touching the cap.
    client.prepare_user(store)?.send_without_preflight().await?;
    let user = client.find_user_address(store, &client.payer());
    deployment.mint_or_transfer_to("fBTC", &user, 0).await?;

    let collateral_amount = 100_000;
    let swap_amount = 1_000_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount)
        .await?;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, swap_amount + 17)
        .await?;

    let size = 5_000 * MARKET_USD_UNIT;
    let price = 400_000 * MARKET_USD_UNIT / 10u128.pow(fbtc.config.decimals as u32);

    // AC4: a swap order is not a position order, so it can carry no builder.
    // Swapping into the long token makes fBTC the final output token, which is
    // the mint the claim vault above was opened for, so the account constraints
    // are satisfied and the kind check is what actually rejects this.
    let (rpc, swap_order) = client
        .market_swap(
            store,
            market_token,
            true,
            &usdg.address,
            swap_amount,
            [market_token],
        )
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%swap_order, %signature, "created a swap order");

    let err = client
        .set_builder_fee(store, &swap_order, &user, 0, None)
        .await?
        .send()
        .await
        .expect_err("should reject a checkpoint onto a swap order");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::BuilderFeeOrderKindNotAllowed.into()),
    );

    let signature = client
        .close_order(&swap_order)?
        .build()
        .await?
        .send()
        .await?;
    tracing::info!(%swap_order, %signature, "cancelled the swap order");

    // AC7: an increase order created without the escrow has no final output
    // token, and so no bucket a fee could ever be paid from.
    let (rpc, bare_order) = client
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
    tracing::info!(%bare_order, %signature, "created an increase order without the escrow");

    // The SDK refuses to build the instruction at all, since it reads the mint
    // off the order and there is none to read.
    let err = client
        .set_builder_fee(store, &bare_order, &user, 0, None)
        .await
        .err()
        .ok_or_eyre("the SDK must refuse to build a checkpoint for an order with no fee token")?;
    tracing::info!(%err, "the SDK rejected the order before building");

    // Forcing a mint past that guard reaches the program's own check, which is
    // the one that has to hold whatever the caller does.
    let err = client
        .set_builder_fee(
            store,
            &bare_order,
            &user,
            0,
            Some(
                SetBuilderFeeHint::builder()
                    .final_output_token(fbtc.address)
                    .build(),
            ),
        )
        .await?
        .send()
        .await
        .expect_err("should reject a checkpoint onto an order with no final output token");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::BuilderFeeFinalOutputTokenNotInitialized.into()),
    );

    let signature = client
        .close_order(&bare_order)?
        .build()
        .await?
        .send()
        .await?;
    tracing::info!(%bare_order, %signature, "cancelled the order without the escrow");

    // AC6 needs an order that passes everything else.
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, collateral_amount)
        .await?;
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
        .prepare_final_output_token_escrow(true)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created a fee-eligible limit increase order");

    // AC6, missing claim vault. The keeper's User Account has no fBTC ATA, and
    // a builder without one would make settlement, and therefore closing the
    // order, fail later; refusing here is what keeps that unreachable.
    keeper.prepare_user(store)?.send_without_preflight().await?;
    let keeper_user = keeper.find_user_address(store, &keeper.payer());
    assert_eq!(
        deployment
            .get_ata_amount(&fbtc.address, &keeper_user)
            .await?,
        None,
        "this case is only meaningful while that claim vault does not exist"
    );

    let err = client
        .set_builder_fee(store, &order, &keeper_user, 0, None)
        .await?
        .send()
        .await
        .expect_err("should reject a builder whose claim vault does not exist");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::AccountNotInitialized.into()),
    );

    // AC6, the controller PDA. The SDK always derives it, so a mismatch has to
    // be built by hand; the one here is the controller of a different mint.
    let wrong_controller =
        find_user_token_controller_address(&user, &usdg.address, client.store_program_id()).0;
    let err = client
        .store_transaction()
        .anchor_accounts(accounts::SetBuilderFee {
            owner: client.payer(),
            store: *store,
            order,
            builder: user,
            final_output_token: fbtc.address,
            claim_vault: get_associated_token_address(&user, &fbtc.address),
            user_token_controller: wrong_controller,
            token_program: anchor_spl::token::ID,
            event_authority: client.store_event_authority(),
            program: *client.store_program_id(),
        })
        .anchor_args(args::SetBuilderFee { expected_factor: 0 })
        .send()
        .await
        .expect_err("should reject a controller that is not the derived PDA");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into()),
    );

    // AC9: the mechanism sits behind `DomainDisabledFlag::BuilderFee`, and this
    // is the only instruction it gates. Everything above rejects on the order or
    // the accounts, so the case is only meaningful with a call that would
    // otherwise be accepted, which is why the same call is repeated afterwards.
    deployment
        .with_builder_fee_disabled(async {
            let err = client
                .set_builder_fee(store, &order, &user, 0, None)
                .await?
                .send()
                .await
                .expect_err("should reject a checkpoint while the feature is disabled");
            assert_eq!(
                gmsol_sdk::Error::from(err).anchor_error_code(),
                Some(CoreError::FeatureDisabled.into()),
            );

            Ok::<_, eyre::Report>(())
        })
        .await?;

    let signature = client
        .set_builder_fee(store, &order, &user, 0, None)
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%order, %signature, "the same checkpoint lands once the feature is back on");

    let signature = client.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "cancelled the order");

    Ok(())
}

/// AC5a: a `CollateralToPnlToken` decrease order can carry no nonzero fee.
///
/// That swap type moves the whole collateral-token output into the pnl token
/// before the receive swap runs, so the bucket a fee would be charged from is
/// empty on every such order. A zero factor stays allowed, since it charges
/// nothing and is how a checkpoint gets cleared.
///
/// A decrease order needs a position, and the position for
/// [`Deployment::DEFAULT_USER`] on this market is opened and fully closed by
/// `balanced_market_order` running concurrently, so this one trades as the
/// exclusive locked user instead. It takes that lock before the builder fee
/// globals; no other test takes them in the opposite order.
#[tokio::test]
async fn set_builder_fee_rejects_collateral_to_pnl_swap() -> eyre::Result<()> {
    /// One percent, in the market factor unit.
    const CAP: u128 = MARKET_USD_UNIT / 100;

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("set_builder_fee_rejects_collateral_to_pnl_swap");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let builder = deployment.user_client(Deployment::USER_1)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();

    let market_token = deployment
        .prepare_market(["fBTC", "fBTC", "USDG"], 1_000_011, 6_000_000_000_007, true)
        .await?;

    let owner = deployment.locked_user_client().await?;

    for signature in [
        owner.prepare_user(store)?.send_without_preflight().await?,
        builder
            .prepare_user(store)?
            .send_without_preflight()
            .await?,
    ] {
        tracing::info!(%signature, "prepared a user account");
    }
    let builder_user = builder.find_user_address(store, &builder.payer());
    deployment
        .mint_or_transfer_to("fBTC", &builder_user, 0)
        .await?;

    // Deliberately small: this market is shared with the other order tests and
    // the position opened here is left behind, so it keeps its footprint on
    // open interest and pool balances negligible.
    let collateral_amount = 10_000;
    deployment
        .mint_or_transfer_to("fBTC", &owner.payer(), collateral_amount * 2)
        .await?;

    let size = 100 * MARKET_USD_UNIT;
    let (rpc, increase) = owner
        .market_increase(store, market_token, true, collateral_amount, true, size)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%increase, %signature, "created the increase order");

    let mut execution = keeper.execute_order(store, oracle, &increase, false)?;
    deployment
        .execute_with_pyth(
            execution
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            true,
            true,
        )
        .await?;
    tracing::info!(%increase, "executed the increase order, opening the position");

    // Never executed, only checkpointed onto and then cancelled.
    let (rpc, order) = owner
        .market_decrease(store, market_token, true, 0, true, size)
        .decrease_position_swap_type(Some(DecreasePositionSwapType::CollateralToPnlToken))
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created a CollateralToPnlToken decrease order");

    deployment
        .with_builder_fee_cap(CAP, async {
            let signature = builder
                .set_builder_fee_factor(store, CAP)?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "the builder advertised its factor");

            let err = owner
                .set_builder_fee(store, &order, &builder_user, CAP, None)
                .await?
                .send()
                .await
                .expect_err("should reject a nonzero fee on a CollateralToPnlToken order");
            assert_eq!(
                gmsol_sdk::Error::from(err).anchor_error_code(),
                Some(CoreError::BuilderFeeSwapTypeNotAllowed.into()),
            );

            // The same order accepts a zero factor, which is what keeps the
            // revocation path open on an order that can never carry a fee.
            let signature = builder
                .set_builder_fee_factor(store, 0)?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "the builder opted back out");

            let signature = owner
                .set_builder_fee(store, &order, &builder_user, 0, None)
                .await?
                .send_without_preflight()
                .await?;
            tracing::info!(%signature, "checkpointed a zero fee on the same order");

            let checkpointed = owner.order(&order).await?;
            assert_eq!(checkpointed.builder, builder_user);
            assert_eq!(checkpointed.builder_fee_factor, 0);

            Ok::<_, eyre::Report>(())
        })
        .await?;

    let signature = owner.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "cancelled the decrease order");

    Ok(())
}
