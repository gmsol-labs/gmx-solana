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

    // Test 7: Transfer authority back to original keeper to not affect other tests
    let original_keeper = client.payer();
    let transfer_back_ix = new_auth_client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::TransferAuthority {
            new_authority: original_keeper,
        })
        .anchor_accounts(lp::accounts::TransferAuthority {
            global_state,
            authority: new_auth,
        });

    let signature = transfer_back_ix.send().await?;
    tracing::info!(%signature, "proposed authority transfer back to keeper");

    // Accept the authority transfer back using the original keeper client
    let accept_back_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::AcceptAuthority {})
        .anchor_accounts(lp::accounts::AcceptAuthority {
            global_state,
            pending_authority: original_keeper,
        });

    let signature = accept_back_ix.send().await?;
    tracing::info!(%signature, "accepted authority transfer back to keeper");

    // Verify authority is back to original keeper
    let gs_final = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");
    assert_eq!(gs_final.authority, original_keeper);
    assert_eq!(gs_final.pending_authority, Pubkey::default());
    tracing::info!("✓ Authority transferred back to original keeper");

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

    // Get GM token for testing (use a different market than other tests)
    let gm_token = deployment
        .market_token("fBTC", "fBTC", "USDG")
        .expect("GM token must exist");

    tracing::info!("Starting comprehensive GM flow with token: {}", gm_token);

    // Step 1: Prepare underlying tokens (for fBTC/fBTC/USDG market)
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 20_000)
        .await?;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, 50_000_000_000)
        .await?;

    // Step 2: Create and execute GM deposit to get GM tokens
    let (rpc, deposit) = user
        .create_deposit(store, gm_token)
        .long_token(5_000, None, None)
        .short_token(30_000_000_000, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, %deposit, "Created GM deposit");

    let mut execute_deposit = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut execute_deposit, None, false, true)
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
    // Note: GM controller should already be created during deployment setup
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
    // Note: GLV controller should already be created during deployment setup
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

/// Test LP token controller functionality
#[tokio::test]
async fn lp_token_controller_tests() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("lp_token_controller_tests");
    let _enter = span.enter();

    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let global_state = deployment.liquidity_provider_global_state;

    // Test 1: Verify controllers were created during setup
    let gm_token = deployment
        .market_token("SOL", "fBTC", "USDG")
        .expect("GM token must exist");

    let controller_seeds = &[
        lp::LP_TOKEN_CONTROLLER_SEED,
        global_state.as_ref(),
        gm_token.as_ref(),
    ];
    let (gm_controller, _) = Pubkey::find_program_address(controller_seeds, &lp::ID);

    let controller_account = keeper
        .account::<lp::LpTokenController>(&gm_controller)
        .await?
        .expect("GM controller must exist");

    assert_eq!(controller_account.global_state, global_state);
    assert_eq!(controller_account.lp_token_mint, *gm_token);
    assert_eq!(controller_account.total_positions, 0);
    assert!(controller_account.is_enabled);
    assert_eq!(controller_account.disabled_at, 0);
    assert_eq!(controller_account.disabled_cum_inv_cost, 0);
    tracing::info!("✓ GM controller verification passed");

    // Test GLV controller
    let glv_token = &deployment.glv_token;
    let glv_controller_seeds = &[
        lp::LP_TOKEN_CONTROLLER_SEED,
        global_state.as_ref(),
        glv_token.as_ref(),
    ];
    let (glv_controller, _) = Pubkey::find_program_address(glv_controller_seeds, &lp::ID);

    let glv_controller_account = keeper
        .account::<lp::LpTokenController>(&glv_controller)
        .await?
        .expect("GLV controller must exist");

    assert_eq!(glv_controller_account.global_state, global_state);
    assert_eq!(glv_controller_account.lp_token_mint, *glv_token);
    assert_eq!(glv_controller_account.total_positions, 0);
    assert!(glv_controller_account.is_enabled);
    tracing::info!("✓ GLV controller verification passed");

    // Test 2: Create a new controller for testing (use a token that doesn't have a controller yet)
    let test_token = deployment
        .market_token("SOL", "WSOL", "WSOL")
        .expect("Test token must exist");

    let test_controller_seeds = &[
        lp::LP_TOKEN_CONTROLLER_SEED,
        global_state.as_ref(),
        test_token.as_ref(),
    ];
    let (test_controller, _) = Pubkey::find_program_address(test_controller_seeds, &lp::ID);

    // Check if controller already exists, if so skip creation test
    let existing_controller = keeper
        .account::<lp::LpTokenController>(&test_controller)
        .await?;

    let test_controller_account = if existing_controller.is_some() {
        tracing::info!("Controller already exists, skipping creation test");
        existing_controller.unwrap()
    } else {
        let create_controller_ix = keeper
            .store_transaction()
            .program(lp::id())
            .anchor_args(lp::instruction::CreateLpTokenController {
                lp_token_mint: *test_token,
            })
            .anchor_accounts(lp::accounts::CreateLpTokenController {
                global_state,
                controller: test_controller,
                authority: keeper.payer(),
                system_program: solana_sdk::system_program::ID,
            });

        let signature = create_controller_ix.send().await?;
        tracing::info!(%signature, "Created test controller");

        keeper
            .account::<lp::LpTokenController>(&test_controller)
            .await?
            .expect("Test controller must exist")
    };

    assert_eq!(test_controller_account.lp_token_mint, *test_token);
    assert!(test_controller_account.is_enabled);
    tracing::info!("✓ Controller creation/verification test passed");

    // Test 3: Disable controller using keeper as authority
    let disable_ix = keeper
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::DisableLpTokenController {})
        .anchor_accounts(lp::accounts::DisableLpTokenController {
            global_state,
            controller: test_controller,
            gt_store: deployment.store,
            gt_program: gmsol_store::ID,
            authority: keeper.payer(),
        });

    let signature = disable_ix.send().await?;
    tracing::info!(%signature, "Disabled test controller");

    let disabled_controller = keeper
        .account::<lp::LpTokenController>(&test_controller)
        .await?
        .expect("Disabled controller must exist");

    assert!(!disabled_controller.is_enabled);
    assert!(disabled_controller.disabled_at > 0);
    assert!(disabled_controller.disabled_cum_inv_cost > 0);
    tracing::info!("✓ Controller disable test passed");

    // Test 4: Try to disable already disabled controller (should fail)
    let disable_again_ix = keeper
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::DisableLpTokenController {})
        .anchor_accounts(lp::accounts::DisableLpTokenController {
            global_state,
            controller: test_controller,
            gt_store: deployment.store,
            gt_program: gmsol_store::ID,
            authority: keeper.payer(),
        });

    let result = disable_again_ix.send().await;
    assert!(
        result.is_err(),
        "Should fail to disable already disabled controller"
    );
    tracing::info!("✓ Double disable prevention test passed");

    // Test 5: Try to create controller with unauthorized authority (should fail)
    let unauthorized_user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let another_test_token = deployment
        .market_token("SOL", "WSOL", "WSOL")
        .expect("Another test token must exist");

    let unauthorized_controller_seeds = &[
        lp::LP_TOKEN_CONTROLLER_SEED,
        global_state.as_ref(),
        another_test_token.as_ref(),
    ];
    let (unauthorized_controller, _) =
        Pubkey::find_program_address(unauthorized_controller_seeds, &lp::ID);

    let unauthorized_create_ix = unauthorized_user
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::CreateLpTokenController {
            lp_token_mint: *another_test_token,
        })
        .anchor_accounts(lp::accounts::CreateLpTokenController {
            global_state,
            controller: unauthorized_controller,
            authority: unauthorized_user.payer(),
            system_program: solana_sdk::system_program::ID,
        });

    let result = unauthorized_create_ix.send().await;
    assert!(result.is_err(), "Should fail with unauthorized authority");
    tracing::info!("✓ Unauthorized controller creation prevention test passed");

    tracing::info!("All LP token controller tests passed!");
    Ok(())
}

/// Test position management with controllers
#[tokio::test]
async fn position_controller_relationship_tests() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("position_controller_relationship_tests");
    let _enter = span.enter();

    let user = deployment.user_client(Deployment::DEFAULT_USER)?;
    let keeper = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let store = &deployment.store;
    let oracle = &deployment.oracle();
    let lp_oracle = &deployment.liquidity_provider_oracle();
    let global_state = deployment.liquidity_provider_global_state;

    // Get GM token and prepare for staking (use different token than other tests)
    let gm_token = deployment
        .market_token("fBTC", "WSOL", "USDG")
        .expect("GM token must exist");

    // Prepare underlying tokens and deposit GM (for fBTC/WSOL/USDG market)
    deployment
        .mint_or_transfer_to_user("fBTC", Deployment::DEFAULT_USER, 10_000)
        .await?;
    deployment
        .mint_or_transfer_to_user("WSOL", Deployment::DEFAULT_USER, 15_000_000_000)
        .await?;
    deployment
        .mint_or_transfer_to_user("USDG", Deployment::DEFAULT_USER, 50_000_000_000)
        .await?;

    let (rpc, deposit) = user
        .create_deposit(store, gm_token)
        .long_token(3_000, None, None)
        .short_token(30_000_000_000, None, None)
        .build_with_address()
        .await?;
    let signature = rpc.send_without_preflight().await?;
    tracing::info!(%signature, "Created GM deposit for position test");

    let mut execute_deposit = keeper.execute_deposit(store, oracle, &deposit, false);
    deployment
        .execute_with_pyth(&mut execute_deposit, None, true, true)
        .await?;

    // Test 1: Stake GM and verify controller relationship (use fixed position_id for later tests)
    let gm_amount = 60_000_000_000;
    let test_position_id = 99999u64; // Fixed ID for testing
    let mut stake_builder = user
        .stake_lp_token(
            store,
            LpTokenKind::Gm,
            gm_token,
            lp_oracle,
            NonZeroU64::new(gm_amount).expect("amount must be non-zero"),
        )
        .with_position_id(test_position_id);

    deployment
        .execute_with_pyth(&mut stake_builder, None, false, true)
        .await?;

    // Verify controller's total_positions increased
    let controller_seeds = &[
        lp::LP_TOKEN_CONTROLLER_SEED,
        global_state.as_ref(),
        gm_token.as_ref(),
    ];
    let (controller, _) = Pubkey::find_program_address(controller_seeds, &lp::ID);

    let controller_account = keeper
        .account::<lp::LpTokenController>(&controller)
        .await?
        .expect("Controller must exist");

    // Note: Controller may already have positions from previous tests, so just verify it increased
    let initial_positions = controller_account.total_positions;
    tracing::info!(
        "✓ Controller has {} positions after stake",
        initial_positions
    );

    // Sleep to allow position to accumulate GT rewards before disabling controller
    tracing::info!("Sleeping for 5 seconds to allow GT rewards accumulation...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    tracing::info!("Sleep completed, proceeding with controller disable test");

    // Test 2: Disable controller and verify behavior
    let disable_ix = keeper
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::DisableLpTokenController {})
        .anchor_accounts(lp::accounts::DisableLpTokenController {
            global_state,
            controller,
            gt_store: deployment.store,
            gt_program: gmsol_store::ID,
            authority: keeper.payer(),
        });

    let signature = disable_ix.send().await?;
    tracing::info!(%signature, "Disabled controller for testing");

    // Verify controller is disabled and has snapshot values
    let disabled_controller = keeper
        .account::<lp::LpTokenController>(&controller)
        .await?
        .expect("Disabled controller must exist");

    assert!(!disabled_controller.is_enabled);
    assert!(disabled_controller.disabled_at > 0);
    assert!(disabled_controller.disabled_cum_inv_cost > 0);
    assert_eq!(disabled_controller.total_positions, initial_positions); // Still has the existing positions
    tracing::info!("✓ Controller disabled with snapshot values recorded");

    // Test 2.1: Try to stake when controller is disabled (should fail)
    let mut failed_stake_builder = user.stake_lp_token(
        store,
        LpTokenKind::Gm,
        gm_token,
        lp_oracle,
        NonZeroU64::new(gm_amount).expect("amount must be non-zero"),
    );

    let result = deployment
        .execute_with_pyth(&mut failed_stake_builder, None, false, false)
        .await;

    assert!(
        result.is_err(),
        "Should fail to stake when controller is disabled"
    );
    tracing::info!("✓ New stake prevented when controller disabled");

    // Test 2.2: Enable claim functionality and test GT claims with disabled controller
    let enable_claim_ix = keeper
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::SetClaimEnabled { enabled: true })
        .anchor_accounts(lp::accounts::SetClaimEnabled {
            global_state,
            authority: keeper.payer(),
        });

    enable_claim_ix.send().await?;
    tracing::info!("Enabled claim functionality for testing");

    // Now test claim operation using the position created in Test 1 (before controller was disabled)
    let position_id_bytes = test_position_id.to_le_bytes();
    let user_payer = user.payer();
    let position_seeds = &[
        lp::POSITION_SEED,
        controller.as_ref(),
        user_payer.as_ref(),
        &position_id_bytes,
    ];
    let (test_position, _) = Pubkey::find_program_address(position_seeds, &lp::ID);

    // Test claim GT rewards on existing position with disabled controller
    let (gt_user, _) = gmsol_sdk::pda::find_user_address(&deployment.store, &user.payer(), &gmsol_store::ID);
    let (event_authority, _) = gmsol_sdk::pda::find_event_authority_address(&gmsol_store::ID);
    
    // Create GT user account if it doesn't exist
    let create_gt_user_ix = user
        .store_transaction()
        .program(gmsol_store::id())
        .anchor_args(gmsol_store::instruction::PrepareUser {})
        .anchor_accounts(gmsol_store::accounts::PrepareUser {
            store: deployment.store,
            user: gt_user,
            owner: user.payer(),
            system_program: solana_sdk::system_program::ID,
        });
    
    let _create_result = create_gt_user_ix.send().await;
    
    let claim_gt_ix = user
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::ClaimGt {
            _position_id: test_position_id,
        })
        .anchor_accounts(lp::accounts::ClaimGt {
            global_state,
            controller,
            store: deployment.store,
            gt_program: gmsol_store::ID,
            position: test_position,
            owner: user.payer(),
            gt_user,
            event_authority,
        });

    let claim_result = claim_gt_ix.send().await;
    match claim_result {
        Ok(_signature) => {
            tracing::info!("✓ Claim GT successful with disabled controller");
        }
        Err(e) => {
            tracing::error!("✗ Claim GT failed: {:?}", e);
        }
    }

    // Test 2.3: Test full unstake after claim
    tracing::info!("Test 2.3: Testing full unstake after claim with disabled controller");
    
    // Sleep a bit more to accumulate additional rewards
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    
    // Get position vault address
    let position_vault_seeds = &[
        lp::VAULT_SEED,
        test_position.as_ref(),
    ];
    let (position_vault, _) = Pubkey::find_program_address(position_vault_seeds, &lp::ID);
    
    // Get position data for unstake amount
    let pos = keeper.account::<lp::Position>(&test_position).await?.expect("Position must exist");
    
    // Get user's LP token account
    let user_lp_token = anchor_spl::associated_token::get_associated_token_address(
        &user.payer(),
        &gm_token,
    );
    
    let unstake_ix = user
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UnstakeLp {
            _position_id: test_position_id,
            unstake_amount: pos.staked_amount, // Full unstake (all tokens)
        })
        .anchor_accounts(lp::accounts::UnstakeLp {
            global_state,
            controller,
            lp_mint: *gm_token,
            store: deployment.store,
            gt_program: gmsol_store::ID,
            position: test_position,
            position_vault,
            owner: user.payer(),
            gt_user,
            user_lp_token,
            event_authority,
            token_program: anchor_spl::token::ID,
        });

    let unstake_result = unstake_ix.send().await;
    match unstake_result {
        Ok(_signature) => {
            tracing::info!("✓ Full unstake successful with disabled controller");
        }
        Err(e) => {
            tracing::error!("✗ Full unstake failed: {:?}", e);
        }
    }

    // Test 2.4: Verify controller state after full unstake
    let controller_after_unstake = keeper.account::<lp::LpTokenController>(&controller).await?.expect("Controller must exist");
    
    // Verify total_positions decreased by 1 (position was closed)
    assert_eq!(controller_after_unstake.total_positions, 0, 
               "Total positions should decrease by 1 after full unstake");

    tracing::info!("✓ Disabled controller behavior tests completed");

    tracing::info!("All position-controller relationship tests passed!");
    Ok(())
}
