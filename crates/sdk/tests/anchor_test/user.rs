use gmsol_programs::gmsol_store::accounts::ReferralCodeV2;
use gmsol_sdk::client::ops::UserOps;
use gmsol_store::CoreError;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn referral() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("referral");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client2 = deployment.user_client(Deployment::USER_1)?;
    let store = &deployment.store;

    let signature = client.prepare_user(store)?.send_without_preflight().await?;
    tracing::info!(%signature, "prepared user account for user 1");

    let signature = client2
        .prepare_user(store)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "prepared user account for user 2");

    let code = ReferralCodeV2::decode("gmso1")?;
    let signature = client
        .initialize_referral_code(store, code)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "initialized referral code for user 1");

    let signature = client2
        .set_referrer(store, code, None)
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "set the referrer of user 2 to user 1");

    // Self-referral.
    let err = client
        .set_referrer(store, code, None)
        .await?
        .send()
        .await
        .expect_err("should throw an error on self-referral");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::SelfReferral.into())
    );

    // Referral code is exclusive.
    client
        .initialize_referral_code(store, code)?
        .send()
        .await
        .expect_err(
            "should throw an error when the referral code has already been set by someone else",
        );

    let signature = client
        .transfer_referral_code(store, &client2.payer(), None)
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "requested to referral code to user 2");

    let signature = client2
        .accept_referral_code(store, code, None)
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "user 2 accepted the referral code");

    // Mutual-referral.
    let err = client
        .set_referrer(store, code, None)
        .await?
        .send()
        .await
        .expect_err("should throw an error on mutal-referral");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::MutualReferral.into())
    );

    Ok(())
}
