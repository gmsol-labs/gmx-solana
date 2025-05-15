use gmsol_sdk::client::ops::ExchangeOps;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn single_token_pool_withdrawal() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("single_token_pool_withdrawal");
    let _enter = span.enter();

    {
        let client = deployment.locked_user_client().await?;
        let amount = 500_000_000;
        deployment
            .mint_or_transfer_to("WSOL", &client.payer(), 2 * amount + 1_000_000)
            .await?;

        let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
        let store = &deployment.store;
        let oracle = &deployment.oracle();
        let market_token = deployment
            .market_token("SOL", "WSOL", "WSOL")
            .expect("must exist");
        let _wsol = deployment.token("WSOL").expect("must exist");

        // Deposit to the single token pool.
        let (rpc, deposit) = client
            .create_deposit(store, market_token)
            .long_token(amount, None, None)
            .short_token(amount, None, None)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%deposit, %signature, "created a deposit");

        let mut builder = keeper.execute_deposit(store, oracle, &deposit, true);
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .await?;

        let market_token_before_withdrawal = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");

        // Withdraw all from the pool.
        let (rpc, withdrawal) = client
            .create_withdrawal(store, market_token, market_token_before_withdrawal)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%withdrawal, %signature, "created a withdrawal");

        let mut builder = keeper.execute_withdrawal(store, oracle, &withdrawal, true);
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .await?;

        let market_token_after_withdrawal = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");
        assert_eq!(market_token_after_withdrawal, 0);
    }

    Ok(())
}

#[tokio::test]
async fn balanced_pool_withdrawal() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("balanced_pool_withdrawal");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let for_swap = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");
    let long_token_amount = 1_000_000;
    let short_token_amount = 6_000_000_000_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, long_token_amount)
        .await?;

    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, short_token_amount)
        .await?;

    let (rpc, deposit) = client
        .create_deposit(store, for_swap)
        .long_token(long_token_amount, None, None)
        .short_token(short_token_amount, None, None)
        .build_with_address()
        .await?;

    let signature = rpc.send().await?;
    tracing::info!(%deposit, %signature, "created a deposit");

    let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut builder, None, true, true)
        .await?;

    {
        let market_token = deployment
            .market_token("fBTC", "fBTC", "USDG")
            .expect("must exist");
        let usdg = deployment.token("USDG").expect("must exist");
        let amount = 10_000_000_000;
        let client = deployment.locked_user_client().await?;

        deployment
            .mint_or_transfer_to("USDG", &client.payer(), 2 * amount)
            .await?;

        // Deposit.
        let (rpc, deposit) = client
            .create_deposit(store, market_token)
            .long_token(amount, Some(&usdg.address), None)
            .long_token_swap_path(vec![*for_swap])
            .short_token(amount, None, None)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%deposit, %signature, "created a deposit");

        let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .await?;

        let market_token_before_withdarwal = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");

        // Withdraw.
        let receiver = keeper.payer();
        let (rpc, withdrawal) = client
            .create_withdrawal(store, market_token, market_token_before_withdarwal)
            .receiver(receiver)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%withdrawal, %signature, %receiver, "created a withdrawal with receiver");

        let mut builder = keeper.execute_withdrawal(store, oracle, &withdrawal, false);
        deployment
            .execute_with_pyth(&mut builder, None, false, true)
            .await?;

        let market_token_after_withdarwal = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");

        assert_eq!(market_token_after_withdarwal, 0);
    }
    Ok(())
}
