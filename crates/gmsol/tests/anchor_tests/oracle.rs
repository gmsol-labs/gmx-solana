use std::time::Duration;

use gmsol::{
    chainlink::{self, pull_oracle::parse_feed_id},
    exchange::ExchangeOps,
    store::oracle::OracleOps,
    types::PriceProviderKind,
    utils::builder::{EstimateFee, MakeBundleBuilder, WithPullOracle},
};

use anchor_client::solana_sdk::pubkey::Pubkey;

use crate::anchor_tests::setup::{current_deployment, Deployment};

#[tokio::test]
async fn update_chainlink_price_feed() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("update_chainlink_price_feed");
    let _enter = span.enter();

    let Ok(chainlink) = chainlink::Client::from_testnet_defaults() else {
        tracing::warn!("the envs for Chainlink Data Streams are not set");
        return Ok(());
    };

    let index = 255;
    let store = &deployment.store;
    let chainlink_verifier_program = &deployment.chainlink_verifier_program;
    let chainlink_access_controller = &deployment.chainlink_access_controller;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let usdg = deployment.token("USDG").unwrap();

    let feed_id_hex = "0x0003dc85e8b01946bf9dfd8b0db860129181eb6105a8c8981d9f28e00b6f60d9";
    let feed_id = Pubkey::new_from_array(parse_feed_id(feed_id_hex)?);

    let (rpc, feed) = keeper.initailize_price_feed(
        store,
        index,
        PriceProviderKind::ChainlinkDataStreams,
        &usdg.address,
        &feed_id,
    );

    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %feed, "created a chainlink feed for USDG");

    let report = chainlink.latest_report(feed_id_hex).await?;

    let signature = keeper
        .update_price_feed_with_chainlink(
            store,
            &feed,
            chainlink_verifier_program,
            chainlink_access_controller,
            &report.report_bytes()?,
        )?
        .send_without_preflight()
        .await?;

    tracing::info!(%signature, %feed, "updated price feed with chainlink report");

    Ok(())
}

#[tokio::test]
async fn use_chainlink_data_streams() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("use_chainlink_data_streams");
    let _enter = span.enter();

    let Ok(chainlink) = chainlink::Client::from_testnet_defaults() else {
        tracing::warn!("the envs for Chainlink Data Streams are not set");
        return Ok(());
    };

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let ctx = deployment.chainlink_pull_oracle(&keeper).await?;

    let chainlink = ctx.make_oracle(&chainlink, &keeper, true);

    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let market_token = deployment.market_token("fETH", "fETH", "USDH").unwrap();

    let amount = 1_000_000;
    deployment
        .mint_or_transfer_to_user("fETH", Deployment::DEFAULT_USER, amount)
        .await?;

    let (rpc, deposit) = client
        .create_deposit(store, market_token)
        .long_token(amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%deposit, %signature, "created a deposit");

    let execute = keeper.execute_deposit(store, oracle, &deposit, false);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let execute = WithPullOracle::new(chainlink, execute, None).await?;
    let mut execute = EstimateFee::new(execute, None);

    let txs = execute.build().await?;

    match txs.send_all(false).await {
        Ok(signatures) => {
            tracing::info!("execute deposit successfully, txs={signatures:#?}");
        }
        Err((signatures, err)) => {
            tracing::error!(%err, "failed to execute deposit, successful txs={signatures:#?}");
        }
    }
    Ok(())
}
