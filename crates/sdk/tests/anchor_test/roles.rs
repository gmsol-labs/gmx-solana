use gmsol_sdk::client::ops::RoleOps;
use gmsol_store::CoreError;

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn enable_and_disable_roles() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("enable_and_disable_roles");
    let _enter = span.enter();

    let store = &deployment.store;
    let admin = &deployment.client;
    let user = &deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let role = "TEST_ROLE";

    // Cannot enable a role by a non-admin.
    let err = user
        .enable_role(store, role)
        .send()
        .await
        .expect_err("should throw error when enabling a role by a non-admin");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotAnAdmin.into())
    );

    // Enable the role.
    let signature = admin
        .enable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled role: {role}");

    // Cannot enable the role if it is already enabled.
    let err = admin
        .enable_role(store, role)
        .send()
        .await
        .expect_err("should throw error when enabling an already enabled role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PreconditionsAreNotMet.into())
    );

    // Cannot disable a role by a non-admin.
    let err = user
        .disable_role(store, role)
        .send()
        .await
        .expect_err("should throw error when disabling a role by a non-admin");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotAnAdmin.into())
    );

    // Disable the role.
    let signature = admin
        .disable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "disabled role: {role}");

    // Cannot disable the role if it is already disabled.
    let err = admin
        .disable_role(store, role)
        .send()
        .await
        .expect_err("should throw error when disabling an already disabled role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PreconditionsAreNotMet.into())
    );

    Ok(())
}

#[tokio::test]
async fn grant_and_revoke_role() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("grant_and_revoke_role");
    let _enter = span.enter();

    let store = &deployment.store;
    let admin = &deployment.client;
    let keeper = &deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let user = deployment.user(Deployment::DEFAULT_USER)?;

    let role = "TEST_ROLE_2";

    // Cannot grant a non-existent role.
    let err = admin
        .grant_role(store, &user, role)
        .send()
        .await
        .expect_err("should throw error when granting a non-existent role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotFound.into())
    );

    // Enable the role.
    let signature = admin
        .enable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled role: {role}");

    // Disable the role.
    let signature = admin
        .disable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "disabled role: {role}");

    // Cannot grant a disabled role.
    let err = admin
        .grant_role(store, &user, role)
        .send()
        .await
        .expect_err("should throw error when granting a disabled role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PreconditionsAreNotMet.into())
    );

    // Enable the role.
    let signature = admin
        .enable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled role: {role}");

    // Cannot grant role by a non-admin.
    let err = keeper
        .grant_role(store, &user, role)
        .send()
        .await
        .expect_err("should throw error when granting a role by a non-admin");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotAnAdmin.into())
    );

    // Grant the role.
    let signature = admin
        .grant_role(store, &user, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "granted role: {role}");

    // Cannot grant the role if the user already has the role.
    let err = admin
        .grant_role(store, &user, role)
        .send()
        .await
        .expect_err("should throw error when granting a role if the user already has the role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PreconditionsAreNotMet.into())
    );

    // Cannot revoke a non-existent role.
    let err = admin
        .revoke_role(store, &user, "NON_EXISTENT_ROLE")
        .send()
        .await
        .expect_err("should throw error when revoking a non-existent role");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotFound.into())
    );

    // Cannot revoke a role by a non-admin.
    let err = keeper
        .revoke_role(store, &user, role)
        .send()
        .await
        .expect_err("should throw error when revoking a role by a non-admin");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotAnAdmin.into())
    );

    // Revoke the role.
    let signature = admin
        .revoke_role(store, &user, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "revoked role: {role}");

    // Grant the role again.
    let signature = admin
        .grant_role(store, &user, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "granted role: {role}");

    // Disable the role.
    let signature = admin
        .disable_role(store, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "disabled role: {role}");

    // The role can be revoked even if it is disabled.
    let signature = admin
        .revoke_role(store, &user, role)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "revoked role: {role}");

    Ok(())
}
