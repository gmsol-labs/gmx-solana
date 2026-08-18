use std::panic::{resume_unwind, AssertUnwindSafe};

use anchor_spl::associated_token::spl_associated_token_account::get_associated_token_address;
use futures_util::FutureExt;
use gmsol_programs::{
    anchor_lang::error::ErrorCode,
    gmsol_store::{
        accounts::ReferralCodeV2,
        client::{accounts, args},
    },
};
use gmsol_sdk::{
    client::ops::{BuilderFeeOps, ConfigOps, UserOps},
    constants::MARKET_USD_UNIT,
};
use gmsol_store::CoreError;
use gmsol_utils::config::FactorKey;
use solana_sdk::{signature::Keypair, signer::Signer};

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

#[tokio::test]
async fn claim_builder_fees() -> eyre::Result<()> {
    /// The token the claim is denominated in.
    const TOKEN: &str = "USDG";
    /// A second token, used for the never-settled case. Nothing in the
    /// suite creates a claim vault for it, which is the point.
    const UNSETTLED_TOKEN: &str = "fBTC";
    /// The balance seeded into the claim vault.
    const AMOUNT: u64 = 1_234_567;

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("claim_builder_fees");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::USER_1)?;
    let other = deployment.user_client(Deployment::DEFAULT_USER)?;
    let store = &deployment.store;
    let token = deployment.token(TOKEN).expect("no such token").address;

    let signature = client.prepare_user(store)?.send_without_preflight().await?;
    tracing::info!(%signature, "prepared user account for the builder");

    let owner = client.payer();
    let user_account = client.find_user_address(store, &owner);
    let claim_vault = get_associated_token_address(&user_account, &token);

    // A destination no other test touches. The users' own ATAs are minted
    // into by tests running concurrently against this same deployment, so
    // asserting on one of those balances would be flaky.
    let destination_owner = Keypair::generate(&mut rand::thread_rng()).pubkey();
    deployment
        .mint_or_transfer_to(TOKEN, &destination_owner, 0)
        .await?;
    let destination = get_associated_token_address(&destination_owner, &token);

    // Seed the claim vault directly. Settlement, the instruction that
    // normally fills it, lands separately; claiming does not care how the
    // balance got there.
    deployment
        .mint_or_transfer_to(TOKEN, &user_account, AMOUNT)
        .await?;
    assert_eq!(
        deployment.get_ata_amount(&token, &user_account).await?,
        Some(AMOUNT)
    );

    // Owner-only: no other signer can claim this vault. The User Account
    // seeds are derived from the signer, so the address passed here cannot
    // be the one the constraint derives.
    let err = other
        .store_transaction()
        .anchor_accounts(accounts::ClaimBuilderFees {
            owner: other.payer(),
            store: *store,
            user_account,
            token_mint: token,
            claim_vault: Some(claim_vault),
            destination,
            user_token_controller: other.find_user_token_controller_address(&user_account, &token),
            token_program: anchor_spl::token::ID,
            event_authority: other.store_event_authority(),
            program: *other.store_program_id(),
        })
        .anchor_args(args::ClaimBuilderFees {})
        .send()
        .await
        .expect_err("should reject a claim signed by anyone but the owner");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert_eq!(
        deployment.get_ata_amount(&token, &user_account).await?,
        Some(AMOUNT),
        "a rejected claim must leave the vault untouched"
    );

    // A transferring claim cannot send the vault to itself: spl-token
    // short-circuits a self-transfer to `Ok(())` without moving any
    // balance, which would emit a claim event for a claim that never
    // happened. Asserted while the vault still holds a balance, since the
    // guard deliberately does not apply to the no-op paths.
    let err = client
        .claim_builder_fees(store, &token, &claim_vault)?
        .send()
        .await
        .expect_err("should reject the claim vault as its own destination");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::InvalidArgument.into())
    );
    assert_eq!(
        deployment.get_ata_amount(&token, &user_account).await?,
        Some(AMOUNT),
        "a rejected claim must leave the vault untouched"
    );

    // The owner claims, and the full balance moves in one go.
    let signature = client
        .claim_builder_fees(store, &token, &destination)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "claimed the builder fees");
    assert_eq!(
        deployment.get_ata_amount(&token, &user_account).await?,
        Some(0)
    );
    assert_eq!(
        deployment
            .get_ata_amount(&token, &destination_owner)
            .await?,
        Some(AMOUNT)
    );

    // Nothing to claim, with the vault present but empty: succeeds and
    // moves nothing.
    let signature = client
        .claim_builder_fees(store, &token, &destination)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "claimed again from an empty vault");
    assert_eq!(
        deployment.get_ata_amount(&token, &user_account).await?,
        Some(0)
    );
    assert_eq!(
        deployment
            .get_ata_amount(&token, &destination_owner)
            .await?,
        Some(AMOUNT),
        "a no-op claim must not move anything"
    );

    // The self-destination guard covers transferring claims only, so it
    // cannot turn a no-op into a failure. This call moves nothing either
    // way, and succeeds.
    let signature = client
        .claim_builder_fees(store, &token, &claim_vault)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "no-op claim into the vault itself");

    // Nothing to claim, with no vault at all: the maximal case of having
    // nothing to claim, reached by omitting the account. The SDK always
    // passes the derived address, so this one is built by hand.
    let unsettled_token = deployment
        .token(UNSETTLED_TOKEN)
        .expect("no such token")
        .address;
    deployment
        .mint_or_transfer_to(UNSETTLED_TOKEN, &destination_owner, 0)
        .await?;
    assert_eq!(
        deployment
            .get_ata_amount(&unsettled_token, &user_account)
            .await?,
        None,
        "the claim vault for this mint must not exist for the case to mean anything"
    );
    let signature = client
        .store_transaction()
        .anchor_accounts(accounts::ClaimBuilderFees {
            owner,
            store: *store,
            user_account,
            token_mint: unsettled_token,
            claim_vault: None,
            destination: get_associated_token_address(&destination_owner, &unsettled_token),
            user_token_controller: client
                .find_user_token_controller_address(&user_account, &unsettled_token),
            token_program: anchor_spl::token::ID,
            event_authority: client.store_event_authority(),
            program: *client.store_program_id(),
        })
        .anchor_args(args::ClaimBuilderFees {})
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "no-op claim with the vault omitted");
    assert_eq!(
        deployment
            .get_ata_amount(&unsettled_token, &user_account)
            .await?,
        None,
        "a no-op claim must not create the vault"
    );

    Ok(())
}
