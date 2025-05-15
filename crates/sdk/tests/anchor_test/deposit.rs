use gmsol_sdk::client::ops::{ExchangeOps, MarketOps};
use gmsol_store::CoreError;
use gmsol_utils::market::MarketConfigKey;
use tracing::Instrument;

use crate::anchor_test::setup::{self, Deployment};

#[tokio::test]
async fn single_token_pool_deposit() -> eyre::Result<()> {
    let deployment = setup::current_deployment().await?;
    let _guard = deployment.use_accounts().await?;

    {
        let span = tracing::info_span!("single_token_pool_deposit");
        let _enter = span.enter();

        let client = deployment.locked_user_client().await?;
        deployment
            .mint_or_transfer_to("WSOL", &client.payer(), 21_000_000)
            .await?;

        let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
        let store = &deployment.store;
        let oracle = &deployment.oracle();
        let market_token = deployment.market_token("SOL", "WSOL", "WSOL").unwrap();
        let wsol = deployment.token("WSOL").expect("must exist");

        let wsol_before = deployment
            .get_user_ata_amount(&wsol.address, None)
            .await?
            .expect("must exist");
        let market_token_before = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .unwrap_or(0);

        let amount = 10_000_000;

        // Create a both sides deposit to single token pool.
        let (rpc, deposit) = client
            .create_deposit(store, market_token)
            .long_token(amount, None, None)
            .short_token(amount, None, None)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%deposit, %signature, "create deposit");

        let wsol_after_creation = deployment
            .get_user_ata_amount(&wsol.address, None)
            .await?
            .expect("must exist");
        let market_token_after_creation = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("market token ata must exist");
        let token_escrow = deployment
            .get_ata_amount(&wsol.address, &deposit)
            .await?
            .expect("token escrow must exist");
        let market_token_escrow = deployment
            .get_ata_amount(market_token, &deposit)
            .await?
            .expect("market token escrow must exist");

        assert_eq!(wsol_after_creation + amount * 2, wsol_before);
        assert_eq!(market_token_after_creation, market_token_before);
        assert_eq!(token_escrow, amount * 2);
        assert_eq!(market_token_escrow, 0);

        // Execute.
        let mut builder = keeper.execute_deposit(store, oracle, &deposit, true);
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .await?;

        let wsol_after_execution = deployment
            .get_user_ata_amount(&wsol.address, None)
            .await?
            .expect("must exist");
        let market_token_after_execution = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("market token ata must exist");
        assert!(deployment
            .get_ata_amount(&wsol.address, &deposit)
            .await?
            .is_none());
        assert!(deployment
            .get_ata_amount(market_token, &deposit)
            .await?
            .is_none());

        assert_eq!(wsol_after_creation, wsol_after_execution);
        assert!(market_token_after_execution >= market_token_after_creation);
        let minted = market_token_after_execution - market_token_after_creation;
        tracing::info!("minted {minted}");
    }

    Ok(())
}

#[tokio::test]
async fn balanced_pool_deposit() -> eyre::Result<()> {
    let deployment = setup::current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("balanced_pool_deposit");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let for_swap = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");
    let long_token_amount = 1_000_002;
    let short_token_amount = 6_000_000_000_001;
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
    tracing::info!(%signature, "created deposit: {deposit}");

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

        let usdg_before = deployment
            .get_user_ata_amount(&usdg.address, None)
            .await?
            .expect("must exist");

        let (rpc, deposit) = client
            .create_deposit(store, market_token)
            .long_token(amount, Some(&usdg.address), None)
            .long_token_swap_path(vec![*for_swap])
            .short_token(amount, None, None)
            .build_with_address()
            .await?;
        let signature = rpc.send().await?;
        tracing::info!(%signature, "created deposit: {deposit}");

        let usdg_after_creation = deployment
            .get_user_ata_amount(&usdg.address, None)
            .await?
            .expect("must exist");
        let market_token_before_exectution = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");
        let usdg_escrow_before_execution = deployment
            .get_ata_amount(&usdg.address, &deposit)
            .await?
            .expect("must exist");
        let market_token_escrow_before_exectuion = deployment
            .get_ata_amount(market_token, &deposit)
            .await?
            .expect("must exist");

        assert_eq!(usdg_after_creation + 2 * amount, usdg_before);
        assert_eq!(usdg_escrow_before_execution, 2 * amount);
        assert_eq!(market_token_escrow_before_exectuion, 0);

        let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
        deployment
            .execute_with_pyth(&mut builder, None, true, true)
            .await?;

        let market_token_after_execution = deployment
            .get_user_ata_amount(market_token, None)
            .await?
            .expect("must exist");
        let usdg_escrow_after_execution =
            deployment.get_ata_amount(&usdg.address, &deposit).await?;
        let market_token_escrow_after_execution =
            deployment.get_ata_amount(market_token, &deposit).await?;

        assert!(market_token_after_execution >= market_token_before_exectution);
        assert!(usdg_escrow_after_execution.is_none());
        assert!(market_token_escrow_after_execution.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn first_deposit() -> eyre::Result<()> {
    let deployment = setup::current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("first_deposit");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let [index_token, long_token, short_token] = Deployment::SELECT_FIRST_DEPOSIT_MARKET;
    let market_token = deployment
        .market_token(index_token, long_token, short_token)
        .expect("must exist");

    let amount = 1_000_000_013;
    let min_amount = 1u64;

    deployment
        .mint_or_transfer_to_user(long_token, Deployment::DEFAULT_USER, amount * 2)
        .await?;

    let signature = keeper
        .update_market_config_by_key(
            store,
            market_token,
            MarketConfigKey::MinTokensForFirstDeposit,
            &(min_amount.into()),
        )?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled first deposit check");

    let (rpc, deposit) = client
        .create_deposit(store, market_token)
        .long_token(amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created first deposit");

    // Invalid first deposit.
    let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
    let err = deployment
        .execute_with_pyth(&mut builder, None, false, false)
        .await
        .expect_err("should throw an error on first deposit with unexpected receiver");
    assert_eq!(
        err.anchor_error_code(),
        Some(CoreError::InvalidReceiverForFirstDeposit.into())
    );

    let (rpc, deposit) = client
        .create_first_deposit(store, market_token)
        .long_token(amount, None, None)
        .min_market_token(min_amount)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created first deposit");
    let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut builder, None, false, false)
        .instrument(tracing::info_span!("execute first deposit", first_deposit=%deposit))
        .await?;

    // Should be able to deposit normally.
    let (rpc, deposit) = client
        .create_deposit(store, market_token)
        .long_token(amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created deposit");
    let mut builder = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut builder, None, false, false)
        .instrument(tracing::info_span!("execute deposit", first_deposit=%deposit))
        .await?;

    Ok(())
}
