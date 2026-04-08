use anchor_spl::token::TokenAccount;
use gmsol_sdk::client::ops::ExchangeOps;
use gmsol_store::CoreError;

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

#[tokio::test]
async fn cross_market_swap_order_mints_withdrawable_phantom_inventory() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("cross_market_swap_order_mints_withdrawable_phantom_inventory");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let user = deployment.user_client(Deployment::USER_1)?;
    let user_pubkey = deployment.user(Deployment::USER_1)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let [src_index, src_long, src_short] = Deployment::SELECT_SWAP_SOURCE_MARKET;
    let [dst_index, dst_long, dst_short] = Deployment::SELECT_SWAP_TARGET_MARKET;
    let qbtc = deployment
        .token(Deployment::TOKEN_FOR_SWAP_TEST)
        .expect("must exist");
    let usdg = deployment.token("USDG").expect("must exist");

    let source_market = deployment
        .prepare_market(
            [src_index, src_long, src_short],
            12_000_000,
            120_000_000_000,
            true,
        )
        .await?;
    let destination_market = deployment
        .market_token(dst_index, dst_long, dst_short)
        .expect("must exist");

    let user_liquidity_amount = 2_000_000;
    deployment
        .mint_or_transfer_to(
            Deployment::TOKEN_FOR_SWAP_TEST,
            &user_pubkey,
            user_liquidity_amount * 2 + 1_000_000,
        )
        .await?;

    let (rpc, deposit) = user
        .create_deposit(store, destination_market)
        .long_token(user_liquidity_amount, None, None)
        .short_token(user_liquidity_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%deposit, %signature, "created user liquidity deposit");

    let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut builder, None, true, true)
        .await?;

    let user_market_token_balance = deployment
        .get_user_ata_amount(destination_market, Some(Deployment::USER_1))
        .await?
        .expect("user market-token ATA must exist");
    assert!(user_market_token_balance > 0);

    let destination_market_balance_before = user
        .market_by_token(store, destination_market)
        .await?
        .state
        .other
        .long_token_balance;
    let qbtc_vault = user.find_market_vault_address(store, &qbtc.address);
    let vault_balance_before = user
        .account::<TokenAccount>(&qbtc_vault)
        .await?
        .unwrap_or_else(|| panic!("{} vault must exist", Deployment::TOKEN_FOR_SWAP_TEST))
        .amount;

    let swap_in_amount = 40_000_000_000;
    deployment
        .mint_or_transfer_to("USDG", &user_pubkey, swap_in_amount)
        .await?;

    let user_qbtc_before_execute = deployment
        .get_user_ata_amount(&qbtc.address, Some(Deployment::USER_1))
        .await?
        .unwrap_or(0);

    let (rpc, order) = user
        .market_swap(
            store,
            destination_market,
            true,
            &usdg.address,
            swap_in_amount,
            [source_market],
        )
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, %swap_in_amount, "created cross-market swap order");

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

    let user_qbtc_after_execute = deployment
        .get_user_ata_amount(&qbtc.address, Some(Deployment::USER_1))
        .await?
        .unwrap_or_else(|| {
            panic!(
                "user {} ATA must exist after executing the order",
                Deployment::TOKEN_FOR_SWAP_TEST
            )
        });
    let order_payout = user_qbtc_after_execute
        .checked_sub(user_qbtc_before_execute)
        .expect("order execution must increase user output balance");
    assert!(order_payout > 0);

    let destination_market_balance_after_execute = user
        .market_by_token(store, destination_market)
        .await?
        .state
        .other
        .long_token_balance;
    assert_eq!(
        destination_market_balance_after_execute, destination_market_balance_before,
        "single-token pool balance must not change during cross-market swap"
    );

    let vault_balance_after_execute = user
        .account::<TokenAccount>(&qbtc_vault)
        .await?
        .unwrap_or_else(|| panic!("{} vault must exist", Deployment::TOKEN_FOR_SWAP_TEST))
        .amount;
    let vault_delta_after_execute = vault_balance_before
        .checked_sub(vault_balance_after_execute)
        .expect("vault must decrease when the order output is paid");

    tracing::info!(
        destination_market_balance_before,
        destination_market_balance_after_execute,
        order_payout,
        vault_delta_after_execute,
        "observed cross-market swap accounting deltas"
    );

    Ok(())
}

#[tokio::test]
async fn create_swap_with_empty_path_should_fail() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let fbtc = deployment.token("fBTC").expect("must exist");
    let market_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 100_000)
        .await?;

    let (rpc, _order) = client
        .market_swap(store, market_token, false, &fbtc.address, 100_000, [])
        .build_with_address()
        .await?;

    let err = rpc
        .send()
        .await
        .expect_err("empty swap_path, can't create order");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::InvalidSwapPathLength.into())
    );

    Ok(())
}
