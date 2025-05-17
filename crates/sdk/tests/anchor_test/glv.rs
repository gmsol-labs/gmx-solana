use gmsol_sdk::client::ops::{ExchangeOps, GlvOps};
use gmsol_store::CoreError;
use tracing::Instrument;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn initialize_glv() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("initialize_glv");
    let _enter = span.enter();

    let store = &deployment.store;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let market_token_1 = deployment
        .market_token("fBTC", "WSOL", "USDG")
        .expect("must exist");
    let market_token_2 = deployment
        .market_token("SOL", "WSOL", "USDG")
        .expect("must exist");

    let index = 255;
    let (rpc, glv_token) = keeper.initialize_glv(store, 255, [*market_token_1, *market_token_2])?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %index, %glv_token, "initialized a new GLV token");

    Ok(())
}

#[tokio::test]
async fn glv_deposit() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_deposit");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let market_token = deployment.market_token("SOL", "fBTC", "USDG").unwrap();
    let market_token_2 = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();

    let long_token_amount = 1_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 3 * long_token_amount + 14)
        .await?;

    // Create and then cancel.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let signature = user
        .close_glv_deposit(&deposit)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %deposit, "cancelled a glv deposit");

    // Create and then execute.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit again");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    // Deposit with another market token.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token_2)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    // Deposit again.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    // Update max value.
    let signature = keeper
        .update_glv_market_config(store, glv_token, market_token, None, Some(1))
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %market_token, "updated market config in the GLV");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    let err = deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            false,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await
        .expect_err("should throw error for exceeding max value");
    assert_eq!(
        err.anchor_error_code(),
        Some(CoreError::ExceedMaxGlvMarketTokenBalanceValue.into())
    );

    // Restore the max value.
    let signature = keeper
        .update_glv_market_config(store, glv_token, market_token, None, Some(0))
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %market_token, "restored market config in the GLV");

    Ok(())
}

#[tokio::test]
async fn glv_withdrawal() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_withdrawal");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let market_token = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();

    let short_token_amount = 1_000 * 100_000_000;

    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            3 * short_token_amount + 17,
        )
        .await?;

    // GLV deposit.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .short_token_deposit(short_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    let glv_amount = 500 * 1_000_000_000;

    // Create and cancel a GLV withdrawal.
    let (rpc, withdrawal) = user
        .create_glv_withdrawal(store, glv_token, market_token, glv_amount)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %withdrawal, "created a glv withdrawal");

    let signature = user
        .close_glv_withdrawal(&withdrawal)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %withdrawal, "cancelled the glv withdrawal");

    // Create and execute a GLV withdrawal.
    let (rpc, withdrawal) = user
        .create_glv_withdrawal(store, glv_token, market_token, glv_amount)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %withdrawal, "created a glv withdrawal again");

    let mut execute = keeper.execute_glv_withdrawal(oracle, &withdrawal, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv withdrawal", glv_withdrawal=%withdrawal))
        .await?;

    Ok(())
}

#[tokio::test]
async fn glv_shift() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("glv_shift");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let glv_token = &deployment.glv_token;
    let to_market_token = deployment.market_token("fBTC", "fBTC", "USDG").unwrap();
    let market_token = deployment.market_token("SOL", "fBTC", "USDG").unwrap();

    let long_token_amount = 1_000;
    let short_token_amount = 1_000 * 100_000_000;

    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 3 * long_token_amount + 37)
        .await?;
    deployment
        .mint_or_transfer_to_user(
            "USDG",
            Deployment::DEFAULT_USER,
            3 * short_token_amount + 37,
        )
        .await?;

    // GLV deposit.
    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(long_token_amount, None, None)
        .short_token_deposit(short_token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "created a glv deposit");

    let mut execute = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv deposit", glv_deposit=%deposit))
        .await?;

    let shift_amount = 500 * 1_000_000_000;

    // Create and cancel a GLV shift.
    let (rpc, shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %shift, "created a glv shift");

    let signature = keeper
        .close_glv_shift(&shift)
        .build()
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, %shift, "cancelled the glv shift");

    // Create and execute a GLV shift.
    let (rpc, shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %shift, "created a glv shift again");

    let mut execute = keeper.execute_glv_shift(oracle, &shift, false);
    deployment
        .execute_with_pyth(
            execute
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing glv shift", glv_shift=%shift))
        .await?;

    let (rpc, _shift) = keeper
        .create_glv_shift(
            store,
            glv_token,
            market_token,
            to_market_token,
            shift_amount,
        )
        .build_with_address()?;
    let err = rpc.send().await.expect_err("should throw an error");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::GlvShiftIntervalNotYetPassed.into())
    );

    Ok(())
}
