use std::panic::{resume_unwind, AssertUnwindSafe};

use futures_util::FutureExt;
use gmsol_programs::{
    anchor_lang::error::ErrorCode,
    gmsol_store::{
        accounts::ReferralCodeV2,
        client::{accounts, args},
    },
};
use gmsol_sdk::{
    client::ops::{ConfigOps, UserOps},
    constants::MARKET_USD_UNIT,
};
use gmsol_store::CoreError;
use gmsol_utils::config::FactorKey;

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

#[tokio::test]
async fn builder_fee_factor() -> eyre::Result<()> {
    /// One percent, in the market factor unit.
    const CAP: u128 = MARKET_USD_UNIT / 100;

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("builder_fee_factor");
    let _enter = span.enter();

    // The keeper holds the `CONFIG_KEEPER` role, which is what may raise the cap.
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let client = deployment.user_client(Deployment::USER_1)?;
    let store = &deployment.store;

    let signature = client.prepare_user(store)?.send_without_preflight().await?;
    tracing::info!(%signature, "prepared user account for the builder");

    let user_address = client.find_user_address(store, &client.payer());

    // Held for the whole test, not just the raised window below: the very first
    // assertion reads the cap as zero, which is only true while no concurrent
    // test has it raised.
    let _cap_lock = Deployment::lock_builder_fee_cap().await;

    // The cap is zero until a config keeper raises it, so the mechanism is
    // closed and no nonzero rate can be advertised yet.
    let err = client
        .set_builder_fee_factor(store, 1)?
        .send()
        .await
        .expect_err("should reject any nonzero factor while the cap is zero");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::BuilderFeeFactorExceedsMaxFactor.into())
    );

    let signature = keeper
        .insert_global_factor_by_key(store, FactorKey::MaxBuilderFeeFactor, &CAP)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "opened the mechanism by raising the cap");

    // Everything that needs the cap raised runs inside this block, so that the
    // restore below is reached however the block ends. The store is a fixture
    // shared with every test in this binary, and they run concurrently, so a
    // cap left raised would leak into unrelated tests. Catching the unwind is
    // what covers a failing assertion; an early return alone would not.
    let result = AssertUnwindSafe(async {
        // Setting the factor exactly at the cap is allowed.
        let signature = client
            .set_builder_fee_factor(store, CAP)?
            .send_without_preflight()
            .await?;
        tracing::info!(%signature, "advertised a builder fee factor at the cap");
        assert_eq!(client.user(&user_address).await?.builder_fee_factor, CAP);

        // One unit above the cap is not.
        let err = client
            .set_builder_fee_factor(store, CAP + 1)?
            .send()
            .await
            .expect_err("should reject a factor above the cap");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(CoreError::BuilderFeeFactorExceedsMaxFactor.into())
        );
        assert_eq!(
            client.user(&user_address).await?.builder_fee_factor,
            CAP,
            "a rejected update must leave the stored factor untouched"
        );

        // Zero always succeeds: it is how a builder opts out.
        let signature = client
            .set_builder_fee_factor(store, 0)?
            .send_without_preflight()
            .await?;
        tracing::info!(%signature, "opted out of the builder fee");
        assert_eq!(client.user(&user_address).await?.builder_fee_factor, 0);

        // Setting the factor on someone else's User Account is unconstructible:
        // the account seeds are derived from the signer, so the address passed
        // here cannot be the one the constraint derives.
        let err = keeper
            .store_transaction()
            .anchor_accounts(accounts::SetBuilderFeeFactor {
                owner: keeper.payer(),
                store: *store,
                user: user_address,
                event_authority: keeper.store_event_authority(),
                program: *keeper.store_program_id(),
            })
            .anchor_args(args::SetBuilderFeeFactor { factor: 0 })
            .send()
            .await
            .expect_err("should reject setting the factor on another user's account");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ErrorCode::ConstraintSeeds.into())
        );

        Ok::<_, eyre::Report>(())
    })
    .catch_unwind()
    .await;

    // Leave the store as it was found.
    let restored = keeper
        .insert_global_factor_by_key(store, FactorKey::MaxBuilderFeeFactor, &0)
        .send_without_preflight()
        .await;

    // The body's failure is the interesting one, so report it before the
    // restore's.
    match result {
        Ok(result) => result?,
        Err(panic) => resume_unwind(panic),
    }

    let signature = restored?;
    tracing::info!(%signature, "closed the mechanism again");

    Ok(())
}
