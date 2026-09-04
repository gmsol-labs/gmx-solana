use gmsol_programs::gmsol_store::accounts::VirtualInventory;
use gmsol_sdk::{
    client::ops::MarketOps, ops::VirtualInventoryOps, utils::zero_copy::ZeroCopy, Client,
};
use gmsol_solana_utils::signer::SignerRef;
use gmsol_store::CoreError;
use solana_sdk::pubkey::Pubkey;

use crate::anchor_test::setup::{current_deployment, Deployment};

/// The index of the virtual inventory this test creates. The shared fixture uses
/// `0` and `1`, so this one is only ever touched here.
const TEST_VIRTUAL_INVENTORY_INDEX: u32 = 1000;

/// Reads the `ref_count` of a virtual inventory account.
async fn ref_count(client: &Client<SignerRef>, virtual_inventory: &Pubkey) -> eyre::Result<u32> {
    let account = client
        .account::<ZeroCopy<VirtualInventory>>(virtual_inventory)
        .await?
        .expect("the virtual inventory must exist");
    Ok(account.0.ref_count)
}

/// A market may only leave a disabled virtual inventory it is currently
/// associated with.
///
/// Without the association check, an unrelated market can decrement `ref_count`,
/// which lets the counter reach zero while other markets still reference the
/// virtual inventory. It can then be closed underneath them, and every operation
/// that resolves the reference (deposits, withdrawals, orders, swaps) fails for
/// markets that did nothing wrong.
#[tokio::test]
async fn leave_disabled_virtual_inventory_requires_association() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("leave_disabled_virtual_inventory_requires_association");
    let _enter = span.enter();

    let store = &deployment.store;
    let token_map = deployment.token_map();
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    // Both markets are created by this test rather than taken from the fixture.
    // The other tests run concurrently, and a market that is joined to a virtual
    // inventory must carry that account in every transaction touching it, so
    // borrowing a shared market would make unrelated tests fail intermittently.
    let index_token = deployment.token("fETH").expect("must exist").address;
    let long_token = deployment.token("USDH").expect("must exist");
    let short_token = deployment.token("fETH").expect("must exist");

    let mut builder = keeper.bundle();

    let (txn, associated_market_token) = keeper
        .create_market(
            store,
            "VI_ASSOCIATED",
            &index_token,
            &long_token.address,
            &short_token.address,
            false,
            Some(&token_map),
        )
        .await?;
    builder.push(txn)?;

    // A different index token, so this is a distinct market: the market token mint
    // is a PDA over the whole (index, long, short) triple.
    let (txn, unassociated_market_token) = keeper
        .create_market(
            store,
            "VI_UNASSOCIATED",
            &long_token.address,
            &long_token.address,
            &short_token.address,
            false,
            Some(&token_map),
        )
        .await?;
    builder.push(txn)?;

    let (txn, virtual_inventory) = keeper
        .create_virtual_inventory_for_swaps(
            store,
            TEST_VIRTUAL_INVENTORY_INDEX,
            long_token.config.decimals,
            short_token.config.decimals,
        )?
        .swap_output(());
    builder.push(txn)?;

    builder
        .build()?
        .send_all(false)
        .await
        .map_err(|(_, err)| err)?;

    let associated_market = keeper.find_market_address(store, &associated_market_token);
    let unassociated_market = keeper.find_market_address(store, &unassociated_market_token);
    tracing::info!(%virtual_inventory, %associated_market, %unassociated_market, "created the test accounts");

    // Associate one of the two markets, then disable the virtual inventory so the
    // `leave_disabled` path becomes the only way out of it.
    let signature = keeper
        .join_virtual_inventory_for_swaps(store, &associated_market, &virtual_inventory, None)
        .await?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "joined the virtual inventory");
    assert_eq!(ref_count(&keeper, &virtual_inventory).await?, 1);

    let signature = keeper
        .disable_virtual_inventory(store, &virtual_inventory)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "disabled the virtual inventory");

    // The market that never joined must not be able to spend the reference.
    let err = keeper
        .leave_disabled_virtual_inventory(store, &unassociated_market, &virtual_inventory)?
        .send()
        .await
        .expect_err("an unassociated market must not be able to leave");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::PreconditionsAreNotMet.into())
    );

    // The rejected call left the counter alone. Had it gone through, the
    // associated market below would have been unable to leave, and the virtual
    // inventory could have been closed while still referenced.
    assert_eq!(ref_count(&keeper, &virtual_inventory).await?, 1);

    // The associated market still leaves normally.
    let signature = keeper
        .leave_disabled_virtual_inventory(store, &associated_market, &virtual_inventory)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "the associated market left the virtual inventory");
    assert_eq!(ref_count(&keeper, &virtual_inventory).await?, 0);

    let market = keeper.market(&associated_market).await?;
    assert_eq!(market.virtual_inventory_for_swaps, Default::default());

    // The counter is only zero once nothing references the account, so closing is
    // now safe.
    let signature = keeper
        .close_virtual_inventory_account(store, &virtual_inventory)?
        .send_without_preflight()
        .await?;
    tracing::info!(%signature, "closed the virtual inventory");

    Ok(())
}
