use gmsol::{store::user::UserOps, types::user::ReferralCode};
use gmsol_store::CoreError;
use rand::random;

use crate::anchor_tests::setup::{current_deployment, Deployment};

#[tokio::test]
async fn referral() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("referral");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client2 = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;

    let signature = client.prepare_user(store)?.send_without_preflight().await?;
    tracing::info!(%signature, "prepared user account for user 1");

    let signature = client2
        .prepare_user(store)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "prepared user account for user 2");

    let code = ReferralCode::decode("gmso1")?;
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
        err.anchor_error_code(),
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

    let code2 = random();
    let signature = client2
        .initialize_referral_code(store, code2)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "initialized referral code for user 2");

    // Mutual-referral.
    let err = client
        .set_referrer(store, code2, None)
        .await?
        .send()
        .await
        .expect_err("should throw an error on mutal-referral");
    assert_eq!(
        err.anchor_error_code(),
        Some(CoreError::MutualReferral.into())
    );

    Ok(())
}
