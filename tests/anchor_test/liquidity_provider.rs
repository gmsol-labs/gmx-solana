use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_liquidity_provider as lp;
use gmsol_sdk::{
    builders::liquidity_provider::LpTokenKind,
    client::ops::liquidity_provider::LiquidityProviderOps,
    ops::{ExchangeOps, GlvOps, MarketOps},
};
use solana_sdk::{pubkey::Pubkey, signer::keypair::Keypair, signer::Signer};
use std::num::NonZeroU64;
use tracing::Instrument;

// Test helpers ----------------------------------------------------------------

// Tests -----------------------------------------------------------------------

#[tokio::test]
async fn liquidity_provider_tests() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("liquidity_provider_tests");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let global_state = deployment.liquidity_provider_global_state;
    let gt_mint = deployment.liquidity_provider_gt_mint.pubkey();

    tracing::info!("Global state: {}", global_state);
    tracing::info!("GT mint: {}", gt_mint);

    // Test 1: Verify initialization
    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");

    assert_eq!(gs.authority, client.payer());
    assert_eq!(gs.gt_mint, gt_mint);
    assert_eq!(gs.min_stake_value, 1_000_000_000_000_000_000_000u128);

    // Verify all buckets have the same initial APY
    let expected_apy = 1_000_000_000_000_000_000u128;
    for (i, &apy) in gs.apy_gradient.iter().enumerate() {
        assert_eq!(
            apy, expected_apy,
            "Bucket {} should have APY {}",
            i, expected_apy
        );
    }
    tracing::info!("✓ Initialization test passed");

    // Test 2: Update min stake value
    let new_min: u128 = 5_000_000_000_000_000_000_000u128; // 5e21
    let update_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UpdateMinStakeValue {
            new_min_stake_value: new_min,
        })
        .anchor_accounts(lp::accounts::UpdateMinStakeValue {
            global_state,
            authority: client.payer(),
        });

    let signature = update_ix.send().await?;
    tracing::info!(%signature, "updated min stake value");

    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");
    assert_eq!(gs.min_stake_value, new_min);
    tracing::info!("✓ Update min stake value test passed");

    // Test 3: Update APY gradient over full range using range updater
    let mut new_grad = [0u128; 53];
    for v in new_grad.iter_mut() {
        *v = 2_000_000_000_000_000_000u128;
    }

    let update_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UpdateApyGradientRange {
            start_bucket: 0u8,
            end_bucket: 52u8,
            apy_values: new_grad.to_vec(),
        })
        .anchor_accounts(lp::accounts::UpdateApyGradient {
            global_state,
            authority: client.payer(),
        });

    let signature = update_ix.send().await?;
    tracing::info!(%signature, "updated APY gradient (full range)");

    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");
    assert_eq!(gs.apy_gradient, new_grad);
    tracing::info!("✓ Update APY gradient (full range) test passed");

    // Test 3.5: Test sparse APY gradient update (Vec-based)
    let bucket_indices: Vec<u8> = vec![0, 10, 25, 52];
    let apy_values: Vec<u128> = vec![
        5_000_000_000_000_000_000u128,  // 5%
        7_000_000_000_000_000_000u128,  // 7%
        3_000_000_000_000_000_000u128,  // 3%
        10_000_000_000_000_000_000u128, // 10%
    ];

    let sparse_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UpdateApyGradientSparse {
            bucket_indices: bucket_indices.clone(),
            apy_values: apy_values.clone(),
        })
        .anchor_accounts(lp::accounts::UpdateApyGradient {
            global_state,
            authority: client.payer(),
        });

    let signature = sparse_ix.send().await?;
    tracing::info!(%signature, "updated sparse APY gradient");

    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");

    // Verify sparse updates were applied correctly
    for (i, &bucket_idx) in bucket_indices.iter().enumerate() {
        let expected_apy = apy_values[i];
        assert_eq!(
            gs.apy_gradient[bucket_idx as usize], expected_apy,
            "Bucket {} should have APY {}",
            bucket_idx, expected_apy
        );
    }
    tracing::info!("✓ Sparse APY gradient update test passed");

    // Test 3.6: Test range APY gradient update
    let range_start = 5u8;
    let range_end = 15u8;
    let range_values = vec![
        6_000_000_000_000_000_000u128,  // Bucket 5: 6%
        6_500_000_000_000_000_000u128,  // Bucket 6: 6.5%
        7_000_000_000_000_000_000u128,  // Bucket 7: 7%
        7_500_000_000_000_000_000u128,  // Bucket 8: 7.5%
        8_000_000_000_000_000_000u128,  // Bucket 9: 8%
        8_500_000_000_000_000_000u128,  // Bucket 10: 8.5%
        9_000_000_000_000_000_000u128,  // Bucket 11: 9%
        9_500_000_000_000_000_000u128,  // Bucket 12: 9.5%
        10_000_000_000_000_000_000u128, // Bucket 13: 10%
        10_500_000_000_000_000_000u128, // Bucket 14: 10.5%
        11_000_000_000_000_000_000u128, // Bucket 15: 11%
    ];

    let range_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UpdateApyGradientRange {
            start_bucket: range_start,
            end_bucket: range_end,
            apy_values: range_values.clone(),
        })
        .anchor_accounts(lp::accounts::UpdateApyGradient {
            global_state,
            authority: client.payer(),
        });

    let signature = range_ix.send().await?;
    tracing::info!(%signature, "updated range APY gradient");

    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");

    // Verify range updates were applied correctly
    for (i, expected_apy) in range_values.iter().enumerate() {
        let bucket_idx = range_start as usize + i;
        assert_eq!(
            gs.apy_gradient[bucket_idx], *expected_apy,
            "Bucket {} should have APY {}",
            bucket_idx, expected_apy
        );
    }
    tracing::info!("✓ Range APY gradient update test passed");

    // Test 4: Transfer and accept authority
    // Use an existing user as the new authority
    let new_auth_client = deployment.user_client(Deployment::USER_1)?;
    let new_auth = new_auth_client.payer();

    let transfer_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::TransferAuthority {
            new_authority: new_auth,
        })
        .anchor_accounts(lp::accounts::TransferAuthority {
            global_state,
            authority: client.payer(),
        });

    let signature = transfer_ix.send().await?;
    tracing::info!(%signature, "proposed authority transfer");

    // Accept the authority transfer using the new authority client
    let accept_ix = new_auth_client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::AcceptAuthority {})
        .anchor_accounts(lp::accounts::AcceptAuthority {
            global_state,
            pending_authority: new_auth,
        });

    let signature = accept_ix.send().await?;
    tracing::info!(%signature, "accepted authority transfer");

    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");
    assert_eq!(gs.authority, new_auth);
    assert_eq!(gs.pending_authority, Pubkey::default());
    tracing::info!("✓ Authority transfer test passed");

    // Test 5: Try unauthorized update (should fail)
    let wrong = Keypair::new();
    let update_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UpdateMinStakeValue {
            new_min_stake_value: 7_000_000_000_000_000_000_000u128,
        })
        .anchor_accounts(lp::accounts::UpdateMinStakeValue {
            global_state,
            authority: wrong.pubkey(),
        });

    let res = update_ix.send().await;
    assert!(res.is_err(), "unauthorized update should fail");
    tracing::info!("✓ Unauthorized update test passed");

    // Test 6: Try transfer to default address (should fail)
    let transfer_ix = new_auth_client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::TransferAuthority {
            new_authority: Pubkey::default(),
        })
        .anchor_accounts(lp::accounts::TransferAuthority {
            global_state,
            authority: new_auth, // Use the new authority we just set
        });

    let res = transfer_ix.send().await;
    assert!(res.is_err(), "transfer to default address should fail");
    tracing::info!("✓ Reject default address test passed");

    tracing::info!("All liquidity provider tests passed!");
    Ok(())
}

/// Comprehensive GM staking flow with real pricing: deposit → value check → stake
#[tokio::test]
async fn comprehensive_gm_flow() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("comprehensive_gm_flow");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let lp_oracle = &deployment.liquidity_provider_oracle();

    // Get GM token for testing
    let gm_token = deployment
        .market_token("SOL", "fBTC", "USDG")
        .expect("GM token must exist");

    tracing::info!("Starting comprehensive GM flow with token: {}", gm_token);

    // Step 1: Prepare underlying tokens
    deployment
        .mint_or_transfer_to_user("WSOL", Deployment::DEFAULT_USER, 15_000_000_000)
        .await?;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 10_000)
        .await?;

    // Step 2: Create and execute GM deposit to get GM tokens
    let (rpc, deposit) = user
        .create_deposit(store, gm_token)
        .long_token(3_000, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "Created GM deposit");

    let mut execute_deposit = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut execute_deposit, None, true, true)
        .instrument(tracing::info_span!("executing GM deposit", deposit=%deposit))
        .await?;

    tracing::info!("GM tokens deposited successfully");

    // Step 3: Get GM token value using real pricing for validation
    let gm_amount = 60_000_000_000; // 60 GM tokens (should be well above minimum stake value)
    let mut price_builder = keeper.get_market_token_value(store, oracle, gm_token, gm_amount);
    deployment
        .execute_with_pyth(&mut price_builder, None, true, true)
        .instrument(tracing::info_span!("getting GM token value", %gm_token, %gm_amount))
        .await?;

    tracing::info!(
        "GM token real value: {} GM tokens validated with real pricing",
        gm_amount as f64 / 1_000_000_000.0
    );

    // Step 4: Stake GM tokens using the SDK builder with real pricing
    let mut stake_builder = user.stake_lp_token(
        store,
        LpTokenKind::Gm,
        gm_token,
        lp_oracle,
        NonZeroU64::new(gm_amount).expect("amount must be non-zero"),
    );

    deployment
        .execute_with_pyth(&mut stake_builder, None, false, true)
        .instrument(tracing::info_span!("staking GM tokens with real pricing"))
        .await?;

    tracing::info!("GM tokens staked successfully with real pricing!");
    tracing::info!("Comprehensive GM flow completed successfully!");
    Ok(())
}

/// Comprehensive GLV staking flow with real pricing: deposit → value check → stake
#[tokio::test]
async fn comprehensive_glv_flow() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("comprehensive_glv_flow");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let lp_oracle = &deployment.liquidity_provider_oracle();
    let glv_token = &deployment.glv_token;

    tracing::info!("Starting comprehensive GLV flow with token: {}", glv_token);

    // Step 1: Prepare underlying tokens for GLV deposit
    let token_amount = 5_000;
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 3 * token_amount + 1000)
        .await?;

    // Step 2: Create and execute GLV deposit to get GLV tokens
    let market_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("Market token must exist");

    let (rpc, deposit) = user
        .create_glv_deposit(store, glv_token, market_token)
        .long_token_deposit(token_amount, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "Created GLV deposit");

    let mut execute_deposit = keeper.execute_glv_deposit(oracle, &deposit, false);
    deployment
        .execute_with_pyth(
            execute_deposit
                .add_alt(deployment.common_alt().clone())
                .add_alt(deployment.market_alt().clone()),
            None,
            false,
            true,
        )
        .instrument(tracing::info_span!("executing GLV deposit", glv_deposit=%deposit))
        .await?;

    tracing::info!("GLV tokens deposited successfully");

    // Step 3: Get GLV token value using real pricing for validation
    let glv_amount = 70_000_000_000; // 70 GLV tokens (should be well above minimum stake value)
    let mut price_builder = keeper.get_glv_token_value(store, oracle, glv_token, glv_amount);
    deployment
        .execute_with_pyth(&mut price_builder, None, true, true)
        .instrument(tracing::info_span!("getting GLV token value", %glv_token, %glv_amount))
        .await?;

    tracing::info!(
        "GLV token real value: {} GLV tokens validated with real pricing",
        glv_amount as f64 / 1_000_000_000.0
    );

    // Step 4: Stake GLV tokens using the SDK builder with real pricing
    let mut stake_builder = user.stake_lp_token(
        store,
        LpTokenKind::Glv,
        glv_token,
        lp_oracle,
        NonZeroU64::new(glv_amount).expect("amount must be non-zero"),
    );

    deployment
        .execute_with_pyth(&mut stake_builder, None, false, true)
        .instrument(tracing::info_span!("staking GLV tokens with real pricing"))
        .await?;

    tracing::info!("GLV tokens staked successfully with real pricing!");
    tracing::info!("Comprehensive GLV flow completed successfully!");
    Ok(())
}
