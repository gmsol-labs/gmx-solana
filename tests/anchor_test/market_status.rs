use gmsol_sdk::client::ops::TokenConfigOps;
use gmsol_store::CoreError;
use gmsol_utils::{
    oracle::PriceProviderKind, price::market_status::MarketStatusFlag, token_config::TokenMapAccess,
};

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn set_feed_config_market_status_flag() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("set_feed_config_market_status_flag");
    let _enter = span.enter();

    let store = &deployment.store;
    let token_map = deployment.token_map();
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let user = deployment.user_client(Deployment::DEFAULT_USER)?;

    let token = deployment.token("fETH").expect("must exist").address;
    let provider = PriceProviderKind::ChainlinkDataStreams;
    let flag = MarketStatusFlag::AllowClosed;

    // Only a MARKET_KEEPER can set the flag.
    let err = user
        .set_feed_config_market_status_flag(store, &token_map, &token, provider, flag, true)
        .send()
        .await
        .expect_err("should throw error when called by a non-market-keeper");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PermissionDenied.into())
    );

    // Enable the flag and read it back from the token map.
    let signature = keeper
        .set_feed_config_market_status_flag(store, &token_map, &token, provider, flag, true)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled the flag");
    let map = keeper.token_map(&token_map).await?;
    let feed_config = map
        .get(&token)
        .expect("must exist")
        .get_feed_config(&provider)
        .expect("must exist");
    assert!(feed_config.market_status_flags().get_flag(flag));

    // Disable the flag and read it back.
    let signature = keeper
        .set_feed_config_market_status_flag(store, &token_map, &token, provider, flag, false)
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "disabled the flag");
    let map = keeper.token_map(&token_map).await?;
    let feed_config = map
        .get(&token)
        .expect("must exist")
        .get_feed_config(&provider)
        .expect("must exist");
    assert!(!feed_config.market_status_flags().get_flag(flag));

    // Setting a flag for a provider without a configured feed fails.
    let err = keeper
        .set_feed_config_market_status_flag(
            store,
            &token_map,
            &token,
            PriceProviderKind::Switchboard,
            flag,
            true,
        )
        .send()
        .await
        .expect_err("should throw error for an unconfigured provider feed");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::NotFound.into())
    );

    // Providers that do not report market status are still accepted.
    let pyth_token = deployment.token("fBTC").expect("must exist").address;
    let signature = keeper
        .set_feed_config_market_status_flag(
            store,
            &token_map,
            &pyth_token,
            PriceProviderKind::Pyth,
            flag,
            true,
        )
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "enabled the flag for a non-status provider");

    Ok(())
}
