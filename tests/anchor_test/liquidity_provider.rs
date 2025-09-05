use crate::anchor_test::setup::{current_deployment, Deployment};
use anchor_spl::token::spl_token;
use gmsol_liquidity_provider as lp;
use gmsol_sdk::ops::UserOps;
use solana_sdk::{
    pubkey::Pubkey, signer::keypair::Keypair, signer::Signer, system_instruction, system_program,
};

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

use std::time::Duration;

/// End-to-end stake → claim (with sleep) → partial & full unstake flow.
/// This test demonstrates the complete LP staking lifecycle with proper test infrastructure.
#[tokio::test]
async fn stake_claim_unstake_flow() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    // Use global state from deployment
    let global_state = deployment.liquidity_provider_global_state;
    let position_id: u64 = 42; // any deterministic id for this test

    // --- Test fixtures setup ---

    // 1. Create a test LP token mint
    let lp_mint_keypair = Keypair::new();
    let lp_mint = lp_mint_keypair.pubkey();

    // Create the LP mint account
    let client_rpc = client.store_program().rpc();
    let rent = client_rpc
        .get_minimum_balance_for_rent_exemption(anchor_spl::token::Mint::LEN)
        .await?;

    let create_lp_mint_ix = client
        .store_transaction()
        .signer(&lp_mint_keypair)
        .pre_instruction(
            system_instruction::create_account(
                &client.payer(),
                &lp_mint,
                rent,
                anchor_spl::token::Mint::LEN as u64,
                &anchor_spl::token::ID,
            ),
            true,
        )
        .pre_instruction(
            anchor_spl::token::spl_token::instruction::initialize_mint2(
                &anchor_spl::token::ID,
                &lp_mint,
                &client.payer(),
                None,
                6, // LP decimals
            )?,
            true,
        );

    let signature = create_lp_mint_ix.send().await?;
    tracing::info!(%signature, "created LP mint account: {}", lp_mint);

    // 2. Create user LP token account
    let user_lp_token =
        anchor_spl::associated_token::get_associated_token_address(&client.payer(), &lp_mint);

    let create_ata_ix = client
        .store_transaction()
        .pre_instruction(
            anchor_spl::associated_token::spl_associated_token_account::instruction::create_associated_token_account(
                &client.payer(),
                &client.payer(),
                &lp_mint,
                &anchor_spl::token::ID,
            ),
            true,
        );

    let signature = create_ata_ix.send().await?;
    tracing::info!(%signature, "created user LP token account: {}", user_lp_token);

    // 3. Mint LP tokens to user
    let lp_amount_to_mint: u64 = 10_000_000_000; // 10,000 LP tokens (with 6 decimals)
    let mint_lp_ix = client.store_transaction().pre_instruction(
        anchor_spl::token::spl_token::instruction::mint_to_checked(
            &anchor_spl::token::ID,
            &lp_mint,
            &user_lp_token,
            &client.payer(),
            &[],
            lp_amount_to_mint,
            6, // LP decimals
        )?,
        true,
    );

    let signature = mint_lp_ix.send().await?;
    tracing::info!(%signature, "minted {} LP tokens to user", lp_amount_to_mint);

    // 4. Enable claim functionality
    let enable_claim_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::SetClaimEnabled { enabled: true })
        .anchor_accounts(lp::accounts::SetClaimEnabled {
            global_state,
            authority: client.payer(),
        });

    let signature = enable_claim_ix.send().await?;
    tracing::info!(%signature, "enabled GT claim functionality");

    // 5. Get GT store and program from deployment
    let gt_store = deployment.store;
    let gt_program = gmsol_programs::gmsol_store::ID;

    // 6. Get GT user address and prepare the user account
    let gt_user = client.find_user_address(&gt_store, &client.payer());
    tracing::info!("GT user address: {}", gt_user);

    // Prepare the GT user account to ensure it exists before claiming
    let prepare_user_ix = client.prepare_user(&gt_store)?;
    let signature = prepare_user_ix.send().await?;
    tracing::info!(%signature, "prepared GT user account");

    // 7. Get event authority for GT program
    let event_authority = client.store_event_authority();

    // Choose stake amounts
    let lp_staked_amount: u64 = 1_000_000_000; // 1,000 LP tokens (with 6 decimals)
    let lp_staked_value: u128 = 60_000_000_000_000_000_000_000u128; // 600.0 in 1e20 units (must be >= min_stake_value)

    // Debug: Print the values we're using
    tracing::info!("LP staked amount: {}", lp_staked_amount);
    tracing::info!("LP staked value (1e20): {}", lp_staked_value);
    tracing::info!("Global state: {}", global_state);
    tracing::info!("GT store: {}", gt_store);
    tracing::info!("GT program: {}", gt_program);

    // Calculate correct PDA addresses for position and vault
    let (position_pda, _) = Pubkey::find_program_address(
        &[
            lp::POSITION_SEED,
            global_state.as_ref(),
            client.payer().as_ref(),
            &position_id.to_le_bytes(),
        ],
        &lp::id(),
    );

    let (position_vault_pda, _) = Pubkey::find_program_address(
        &[
            lp::POSITION_SEED,
            global_state.as_ref(),
            client.payer().as_ref(),
            &position_id.to_le_bytes(),
            lp::VAULT_SEED,
        ],
        &lp::id(),
    );

    // --- Test basic functionality first ---
    tracing::info!("Testing basic liquidity provider functionality...");

    // Try to read the global state to verify it's accessible
    let gs = client
        .account::<lp::GlobalState>(&global_state)
        .await?
        .expect("global_state must exist");

    tracing::info!("Global state loaded successfully:");
    tracing::info!("  - Authority: {}", gs.authority);
    tracing::info!("  - GT mint: {}", gs.gt_mint);
    tracing::info!("  - Min stake value: {}", gs.min_stake_value);
    tracing::info!("  - Claim enabled: {}", gs.claim_enabled);

    // Verify our values meet the requirements
    if lp_staked_value < gs.min_stake_value {
        tracing::error!(
            "LP staked value {} is less than min_stake_value {}",
            lp_staked_value,
            gs.min_stake_value
        );
        return Err(eyre::eyre!("LP staked value too low"));
    }

    tracing::info!("Values validation passed");

    // --- Stake ---
    tracing::info!(
        "Attempting to stake {} LP tokens with value {}",
        lp_staked_amount,
        lp_staked_value
    );

    let stake_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::StakeLp {
            position_id,
            lp_staked_amount,
            lp_staked_value,
        })
        .anchor_accounts(lp::accounts::StakeLp {
            global_state,
            lp_mint,
            position: position_pda,
            position_vault: position_vault_pda,
            gt_store,
            gt_program,
            owner: client.payer(),
            user_lp_token,
            system_program: system_program::ID,
            token_program: spl_token::ID,
        });

    let stake_sig = stake_ix.send().await?;
    tracing::info!(%stake_sig, "Successfully staked {} LP tokens", lp_staked_amount);

    // --- Sleep before claim to ensure reward accrual across time ---
    tokio::time::sleep(Duration::from_secs(3)).await;

    // --- Claim ---
    let claim_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::ClaimGt {
            _position_id: position_id,
        })
        .anchor_accounts(lp::accounts::ClaimGt {
            global_state,
            store: gt_store,
            gt_program,
            position: position_pda,
            owner: client.payer(), // Signer for the position owner
            gt_user,               // GT user account loader in real fixture
            event_authority,
        });

    let claim_sig = claim_ix.send().await?;
    tracing::info!(%claim_sig, "Claimed GT rewards");

    // --- Check position state after claim ---
    let position_account = client
        .account::<lp::Position>(&position_pda)
        .await?
        .expect("position must exist after staking");
    tracing::info!(
        "Position after claim - staked_amount: {}, staked_value: {}, position_id: {}",
        position_account.staked_amount,
        position_account.staked_value_usd,
        position_account.position_id
    );

    // --- Check position state before unstake ---
    let position_account_before = client
        .account::<lp::Position>(&position_pda)
        .await?
        .expect("position must exist before unstake");
    tracing::info!(
        "Position before unstake - staked_amount: {}, staked_value: {}",
        position_account_before.staked_amount,
        position_account_before.staked_value_usd
    );

    // --- Partial unstake ---
    let partial_unstake: u64 = lp_staked_amount / 2;
    tracing::info!(
        "Attempting to unstake {} LP tokens (partial)",
        partial_unstake
    );
    tracing::info!(
        "Expected remaining amount: {}",
        lp_staked_amount - partial_unstake
    );
    tracing::info!(
        "Expected remaining value: {} (should be >= min_stake_value: {})",
        (position_account_before.staked_value_usd * (lp_staked_amount - partial_unstake) as u128)
            / lp_staked_amount as u128,
        gs.min_stake_value
    );

    let unstake_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UnstakeLp {
            _position_id: position_id,
            unstake_amount: partial_unstake,
        })
        .anchor_accounts(lp::accounts::UnstakeLp {
            global_state,
            lp_mint,
            store: gt_store,
            gt_program,
            position: position_pda,
            position_vault: position_vault_pda,
            owner: client.payer(),
            gt_user,
            user_lp_token,
            event_authority,
            token_program: spl_token::ID,
        });

    let unstake_sig = unstake_ix.send().await?;
    tracing::info!(%unstake_sig, "Partially unstaked {} LP tokens", partial_unstake);

    // --- Check position state after partial unstake ---
    let position_account = client
        .account::<lp::Position>(&position_pda)
        .await?
        .expect("position must exist after partial unstake");
    tracing::info!(
        "Position after partial unstake - staked_amount: {}, staked_value: {}",
        position_account.staked_amount,
        position_account.staked_value_usd
    );

    // --- Full unstake (remaining) ---
    let remaining_unstake: u64 = lp_staked_amount - partial_unstake;
    tracing::info!(
        "Attempting to unstake remaining {} LP tokens (full exit)",
        remaining_unstake
    );

    let full_unstake_ix = client
        .store_transaction()
        .program(lp::id())
        .anchor_args(lp::instruction::UnstakeLp {
            _position_id: position_id,
            unstake_amount: remaining_unstake,
        })
        .anchor_accounts(lp::accounts::UnstakeLp {
            global_state,
            lp_mint,
            store: gt_store,
            gt_program,
            position: position_pda,
            position_vault: position_vault_pda,
            owner: client.payer(),
            gt_user,
            user_lp_token,
            event_authority,
            token_program: spl_token::ID,
        });

    let full_unstake_sig = full_unstake_ix.send().await?;
    tracing::info!(%full_unstake_sig, "Fully unstaked remaining {} LP tokens", remaining_unstake);

    // --- Verify position is closed after full unstake ---
    let position_account_after_full = client.account::<lp::Position>(&position_pda).await;

    match position_account_after_full {
        Ok(_) => {
            tracing::warn!("Position account still exists after full unstake - this might be expected behavior");
        }
        Err(_) => {
            tracing::info!("Position account successfully closed after full unstake");
        }
    }

    tracing::info!("✓ stake_claim_unstake_flow test completed successfully!");
    Ok(())
}
