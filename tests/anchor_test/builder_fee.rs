use anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address;
use anchor_spl::token::ID as TOKEN_PROGRAM_ID;
use gmsol_sdk::{
    client::ops::{BuilderFeeOps, ExchangeOps, TokenAccountOps, UserOps},
    constants::MARKET_USD_UNIT,
};
use gmsol_store::CoreError;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn settle_builder_fee_no_op() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("settle_builder_fee_no_op");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;

    let market_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("must exist");

    let collateral_amount = 100 * 100_000_000;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, collateral_amount)
        .await?;

    // The settler's User Account is used as the placeholder builder account
    // for orders without a builder.
    let signature = client.prepare_user(store)?.send_without_preflight().await?;
    tracing::info!(%signature, "prepared user account");

    let size = 100 * MARKET_USD_UNIT;
    let (rpc, order) = client
        .market_increase(store, market_token, false, collateral_amount, true, size)
        .build_with_address()
        .await?;
    let signature = rpc.send().await?;
    tracing::info!(%order, %signature, "created an increase position order without a builder");

    // Settling an order without a builder fee is an explicit no-op.
    let signature = client
        .settle_builder_fee(store, &order, None)
        .await?
        .send()
        .await?;
    tracing::info!(%order, %signature, "settled the builder fee (no-op)");

    let account = client.order(&order).await?;
    assert_eq!(account.builder_fee_amount, 0);

    // An order with no unsettled builder fee can be closed.
    let signature = client.close_order(&order)?.build().await?.send().await?;
    tracing::info!(%order, %signature, "order cancelled");

    Ok(())
}

#[tokio::test]
async fn claim_builder_fees() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("claim_builder_fees");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let builder = deployment.user_client(Deployment::USER_1)?;
    let store = &deployment.store;
    let usdg = deployment.token("USDG").expect("must exist").address;

    let signature = builder
        .prepare_user(store)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "prepared the builder's user account");
    let builder_user = builder.find_user_address(store, &builder.payer());

    // Fund the claim vault directly, standing in for settled builder fees
    // (charging a real fee requires order execution, which cannot record a
    // builder fee until the execution-side changes land).
    let claim_amount = 5_000_000;
    deployment
        .mint_or_transfer_to("USDG", &builder_user, claim_amount)
        .await?;

    let signature = builder
        .prepare_associated_token_account(&usdg, &TOKEN_PROGRAM_ID, None)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "prepared the receiver vault");
    let receiver_vault = get_associated_token_address(&builder.payer(), &usdg);

    // The claim requires the per-token controller to have been initialized.
    builder
        .claim_builder_fees(store, &usdg, &receiver_vault)
        .send()
        .await
        .expect_err("should throw an error when the controller is not initialized");

    // Only MARKET_KEEPER can initialize the controller.
    let err = builder
        .initialize_builder_fee_token_controller(store, &usdg)
        .send()
        .await
        .expect_err("should throw an error when the authority is not a MARKET_KEEPER");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PermissionDenied.into())
    );

    let signature = keeper
        .initialize_builder_fee_token_controller(store, &usdg)
        .send()
        .await?;
    tracing::info!(%signature, "initialized the builder fee token controller");

    let receiver_before = deployment
        .get_ata_amount(&usdg, &builder.payer())
        .await?
        .unwrap_or(0);

    let signature = builder
        .claim_builder_fees(store, &usdg, &receiver_vault)
        .send()
        .await?;
    tracing::info!(%signature, "claimed the builder fees");

    let claim_vault_amount = deployment
        .get_ata_amount(&usdg, &builder_user)
        .await?
        .expect("claim vault must exist");
    assert_eq!(claim_vault_amount, 0);
    let receiver_after = deployment
        .get_ata_amount(&usdg, &builder.payer())
        .await?
        .expect("receiver vault must exist");
    assert_eq!(receiver_after, receiver_before + claim_amount);

    // Claiming with an empty claim vault is an explicit no-op.
    let signature = builder
        .claim_builder_fees(store, &usdg, &receiver_vault)
        .send()
        .await?;
    tracing::info!(%signature, "claimed the builder fees again (no-op)");

    let receiver_amount = deployment
        .get_ata_amount(&usdg, &builder.payer())
        .await?
        .expect("receiver vault must exist");
    assert_eq!(receiver_amount, receiver_after);

    Ok(())
}
