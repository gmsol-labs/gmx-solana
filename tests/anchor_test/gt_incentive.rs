use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_gt_incentive as gt_incentive;
use gmsol_sdk::client::ops::UserOps;
use gmsol_store::CoreError;
use solana_sdk::{pubkey::Pubkey, system_program};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

/// Per-file serialization lock.
///
/// All airdrop tests share the same on-chain `OperatorEntry` (under
/// `DEFAULT_USER`). Updating `T`/`N` on it is racy when tokio runs the
/// tests in parallel, which corrupted `test_create_airdrop_rejects_short_duration`.
/// This lock forces them to run one at a time within this file. Tests in
/// other files are unaffected.
fn serial_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// PDA helpers
// ============================================================================

fn airdrop_config_pda(store: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"airdrop_config", store.as_ref()], &gt_incentive::ID).0
}

fn gt_authority_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"gt_authority"], &gt_incentive::ID).0
}

fn store_event_authority_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], &gmsol_store::ID).0
}

fn airdrop_pda(store: &Pubkey, operator: &Pubkey, nonce: &[u8; 8]) -> Pubkey {
    Pubkey::find_program_address(
        &[b"airdrop", store.as_ref(), operator.as_ref(), nonce],
        &gt_incentive::ID,
    )
    .0
}

fn airdrop_target_pda(airdrop: &Pubkey, recipient: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"airdrop_target", airdrop.as_ref(), recipient.as_ref()],
        &gt_incentive::ID,
    )
    .0
}

fn user_pda(store: &Pubkey, owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"user", store.as_ref(), owner.as_ref()],
        &gmsol_store::ID,
    )
    .0
}

// ============================================================================
// tests
// ============================================================================

/// Smoke test that confirms the deployment-level setup ran cleanly
/// (admin called `initialize_airdrop_config` and granted `GT_CONTROLLER`
/// to the program's `gt_authority` PDA).
#[tokio::test]
async fn test_gt_incentive() -> eyre::Result<()> {
    let _deployment = current_deployment().await?;
    Ok(())
}

/// End-to-end happy path: admin → operator → gov → user.
///
/// Walks through every one of the 7 instructions in the order they are
/// expected to be called in production (S1.1 → S1.5 plus the two admin
/// instructions that bootstrap the operator).
#[tokio::test]
async fn test_full_airdrop_flow() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_full_airdrop_flow");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;

    let gov_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let gov = gov_client.payer();
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient_client = deployment.user_client(Deployment::USER_1)?;
    let recipient = recipient_client.payer();

    let airdrop_config = airdrop_config_pda(&store);
    let gt_authority = gt_authority_pda();
    let store_event_authority = store_event_authority_pda();

    // ---- Step 1: admin enables the operator ----
    let timelock_secs = 3u64;
    let max_airdrop_amount = 1_000_000u64;
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs,
            max_airdrop_amount,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // ---- Step 2: operator creates the airdrop (S1.1) ----
    let nonce: [u8; 8] = [0xa1, 0xb2, 0xc3, 0xd4, 0, 0, 0, 0];
    let duration = 60u64;
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop { nonce, duration })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // ---- Step 3: operator adds a target (S1.2) ----
    let amount = 100u64;
    let airdrop_target = airdrop_target_pda(&airdrop, &recipient);
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget { recipient, amount })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // ---- Step 4: operator marks complete (S1.3) ----
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CompleteAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CompleteAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
        })
        .send()
        .await?;

    // ---- Step 5: gov approves (S1.4) ----
    gov_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ApproveAirdrop {})
        .anchor_accounts(gt_incentive::accounts::ApproveAirdrop {
            authority: gov,
            store,
            airdrop_config,
            airdrop,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // ---- Step 6: wait for the timelock to elapse ----
    sleep(Duration::from_secs(timelock_secs + 2)).await;

    // ---- Step 7: recipient prepares their UserHeader and claims (S1.5) ----
    recipient_client
        .prepare_user(&store)?
        .send_without_preflight()
        .await?;

    recipient_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ClaimAirdropTarget {})
        .anchor_accounts(gt_incentive::accounts::ClaimAirdropTarget {
            claimer: recipient,
            store,
            airdrop,
            airdrop_target,
            gt_authority,
            user: user_pda(&store, &recipient),
            store_event_authority,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    Ok(())
}

/// S1.1 boundary: `duration < 2T` must be rejected.
///
/// Rationale: the requirement explicitly states "duration must be ≥ 2T".
/// This is one of the few hard numeric constraints in the spec, and the
/// boundary value (2T-1) is exactly where off-by-one bugs hide.
#[tokio::test]
async fn test_create_airdrop_rejects_short_duration() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_create_airdrop_rejects_short_duration");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();

    let airdrop_config = airdrop_config_pda(&store);

    // Operator gets T = 10s.
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 10,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // Try duration = 19, which is just below 2T = 20. Must fail.
    let nonce: [u8; 8] = [0xb1, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    let err = operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop { nonce, duration: 19 })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("create_airdrop with duration < 2T should fail");

    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::InvalidArgument.into())
    );
    Ok(())
}

/// S1.2 boundary: cumulative target amount > N must be rejected.
///
/// Rationale: N is the per-airdrop GT cap and the last line of defense
/// against a compromised operator. The check is on the *cumulative* total,
/// not single amounts — easy to write incorrectly. This test pushes the
/// running total past the limit on the second target.
#[tokio::test]
async fn test_add_target_exceeds_max_amount() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_add_target_exceeds_max_amount");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient1 = deployment.user_client(Deployment::USER_1)?.payer();
    // Any pubkey works as a "recipient" for upload — claim is what requires
    // a real wallet. We just need a different address.
    let recipient2 = admin.payer();

    let airdrop_config = airdrop_config_pda(&store);

    // Operator's per-airdrop cap is N = 1_000.
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xb2, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // First target: amount = 600. Should succeed (running total = 600 ≤ 1000).
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient: recipient1,
            amount: 600,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient1),
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // Second target: amount = 500. Running total would be 1100 > 1000. Must fail.
    let err = operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient: recipient2,
            amount: 500,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient2),
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("add_target exceeding cumulative max should fail");

    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::InvalidArgument.into())
    );
    Ok(())
}

/// PDA uniqueness: adding the same recipient twice in one airdrop must fail.
///
/// Rationale: our entire defense against duplicate uploads is the PDA
/// derivation `[airdrop_target, airdrop, recipient]`. There is no other
/// duplicate check. If this assumption silently breaks (e.g. someone
/// changes the seeds), nothing else stops a recipient from being added
/// twice and double-funded.
#[tokio::test]
async fn test_duplicate_recipient_rejected() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_duplicate_recipient_rejected");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient = deployment.user_client(Deployment::USER_1)?.payer();

    let airdrop_config = airdrop_config_pda(&store);

    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xb3, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // First add: success.
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient),
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // Second add for the same recipient: must fail (PDA already initialized).
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient),
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("adding the same recipient twice should fail");

    Ok(())
}

/// S1.5 timelock: claiming before `claimable_at` must fail.
///
/// Rationale: the timelock is the post-approval review window. If an attacker
/// could bypass it, a malicious gov + operator pair could approve and drain
/// in the same block, leaving no time for the community to react. This test
/// holds approve and claim back-to-back in the same test, with no sleep.
#[tokio::test]
async fn test_claim_before_timelock_fails() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_claim_before_timelock_fails");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let gov_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let gov = gov_client.payer();
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient_client = deployment.user_client(Deployment::USER_1)?;
    let recipient = recipient_client.payer();

    let airdrop_config = airdrop_config_pda(&store);
    let gt_authority = gt_authority_pda();
    let store_event_authority = store_event_authority_pda();

    // T = 60s — long enough that a no-sleep claim is comfortably before
    // claimable_at, and immune to any small clock drift.
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 60,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xb4, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    let airdrop_target = airdrop_target_pda(&airdrop, &recipient);

    // Build airdrop, add target, complete, approve.
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 600,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CompleteAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CompleteAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
        })
        .send()
        .await?;
    gov_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ApproveAirdrop {})
        .anchor_accounts(gt_incentive::accounts::ApproveAirdrop {
            authority: gov,
            store,
            airdrop_config,
            airdrop,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // Recipient prepares user but tries to claim immediately.
    recipient_client
        .prepare_user(&store)?
        .send_without_preflight()
        .await?;

    let err = recipient_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ClaimAirdropTarget {})
        .anchor_accounts(gt_incentive::accounts::ClaimAirdropTarget {
            claimer: recipient,
            store,
            airdrop,
            airdrop_target,
            gt_authority,
            user: user_pda(&store, &recipient),
            store_event_authority,
            store_program: gmsol_store::ID,
        })
        .send()
        .await
        .expect_err("claim before claimable_at should fail");

    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropTimelockNotElapsed.into())
    );
    Ok(())
}

/// S1.5 anti-double-spend: claiming twice from the same target must fail.
///
/// Rationale: this is the single most important security invariant in the
/// whole contract. If it leaks, anyone can mint unlimited GT. The current
/// implementation marks `is_claimed = true` *before* the mint CPI; this
/// test pins that ordering so no future change can flip it.
#[tokio::test]
async fn test_claim_double_spend_rejected() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_claim_double_spend_rejected");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let gov_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let gov = gov_client.payer();
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient_client = deployment.user_client(Deployment::USER_1)?;
    let recipient = recipient_client.payer();

    let airdrop_config = airdrop_config_pda(&store);
    let gt_authority = gt_authority_pda();
    let store_event_authority = store_event_authority_pda();

    let timelock_secs = 3u64;
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xb5, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);
    let airdrop_target = airdrop_target_pda(&airdrop, &recipient);

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CompleteAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CompleteAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
        })
        .send()
        .await?;
    gov_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ApproveAirdrop {})
        .anchor_accounts(gt_incentive::accounts::ApproveAirdrop {
            authority: gov,
            store,
            airdrop_config,
            airdrop,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    sleep(Duration::from_secs(timelock_secs + 2)).await;

    recipient_client
        .prepare_user(&store)?
        .send_without_preflight()
        .await?;

    // First claim succeeds.
    recipient_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ClaimAirdropTarget {})
        .anchor_accounts(gt_incentive::accounts::ClaimAirdropTarget {
            claimer: recipient,
            store,
            airdrop,
            airdrop_target,
            gt_authority,
            user: user_pda(&store, &recipient),
            store_event_authority,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // Second claim must fail.
    let err = recipient_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ClaimAirdropTarget {})
        .anchor_accounts(gt_incentive::accounts::ClaimAirdropTarget {
            claimer: recipient,
            store,
            airdrop,
            airdrop_target,
            gt_authority,
            user: user_pda(&store, &recipient),
            store_event_authority,
            store_program: gmsol_store::ID,
        })
        .send()
        .await
        .expect_err("second claim should fail");

    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropTargetAlreadyClaimed.into())
    );
    Ok(())
}

/// Cancel happy path: every subsequent state-changing op is blocked.
///
/// Rationale: cancel must be a sticky terminal state before approval, not
/// a transient "paused" mode. If any of add_target / complete / approve
/// could re-activate a cancelled campaign, the operator's "I changed my
/// mind" escape hatch becomes a footgun.
#[tokio::test]
async fn test_cancel_blocks_subsequent_ops() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_cancel_blocks_subsequent_ops");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let gov_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let gov = gov_client.payer();
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient = deployment.user_client(Deployment::USER_1)?.payer();
    // A second pubkey for the post-cancel add attempt — any address works,
    // we only need it to differ from `recipient` so the PDA derivation is new.
    let recipient_2 = admin.payer();

    let airdrop_config = airdrop_config_pda(&store);

    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xc1, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient),
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // Cancel: this is the new instruction under test.
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CancelAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CancelAirdrop {
            operator,
            store,
            airdrop,
        })
        .send()
        .await?;

    // After cancel, add_target must fail.
    let err = operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient: recipient_2,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient_2),
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("add_target after cancel should fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropCancelled.into())
    );

    // complete_airdrop must fail.
    let err = operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CompleteAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CompleteAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
        })
        .send()
        .await
        .expect_err("complete after cancel should fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropCancelled.into())
    );

    // approve_airdrop must fail.
    let err = gov_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ApproveAirdrop {})
        .anchor_accounts(gt_incentive::accounts::ApproveAirdrop {
            authority: gov,
            store,
            airdrop_config,
            airdrop,
            store_program: gmsol_store::ID,
        })
        .send()
        .await
        .expect_err("approve after cancel should fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropCancelled.into())
    );

    Ok(())
}

/// Cancel must fail once the airdrop has been approved.
///
/// Rationale: approval is the handoff point from operator to gov. After
/// that, only gov should be able to halt the campaign — the operator
/// cannot retroactively undo a governance-sanctioned distribution.
#[tokio::test]
async fn test_cancel_after_approve_fails() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_cancel_after_approve_fails");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let gov_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let gov = gov_client.payer();
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let recipient = deployment.user_client(Deployment::USER_1)?.payer();

    let airdrop_config = airdrop_config_pda(&store);

    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xc2, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 600,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::AddAirdropTarget {
            recipient,
            amount: 100,
        })
        .anchor_accounts(gt_incentive::accounts::AddAirdropTarget {
            operator,
            store,
            airdrop_config,
            airdrop,
            airdrop_target: airdrop_target_pda(&airdrop, &recipient),
            system_program: system_program::ID,
        })
        .send()
        .await?;
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CompleteAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CompleteAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
        })
        .send()
        .await?;
    gov_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::ApproveAirdrop {})
        .anchor_accounts(gt_incentive::accounts::ApproveAirdrop {
            authority: gov,
            store,
            airdrop_config,
            airdrop,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // Operator attempts to cancel an approved airdrop: must fail.
    let err = operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CancelAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CancelAirdrop {
            operator,
            store,
            airdrop,
        })
        .send()
        .await
        .expect_err("cancel after approve should fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::AirdropAlreadyApproved.into())
    );

    Ok(())
}

/// Only the original operator may cancel an airdrop.
///
/// Rationale: the PDA seeds embed `operator.key()`, so a different signer
/// derives a different PDA. This is the entire auth boundary — without
/// it any funded address could cancel anyone else's campaign. This test
/// pins the boundary so a future refactor can't accidentally drop the
/// operator out of the seeds.
#[tokio::test]
async fn test_non_operator_cancel_rejected() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_non_operator_cancel_rejected");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();
    let attacker_client = deployment.user_client(Deployment::USER_1)?;
    let attacker = attacker_client.payer();

    let airdrop_config = airdrop_config_pda(&store);

    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xc3, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // Attacker signs as themselves and passes their own key as operator.
    // The PDA derived from (store, attacker.key(), nonce) does not match
    // the actual airdrop account, so Anchor's seed constraint rejects.
    let err = attacker_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CancelAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CancelAirdrop {
            operator: attacker,
            store,
            airdrop,
        })
        .send()
        .await
        .expect_err("non-operator cancel should fail");
    // 2006 = anchor_lang::error::ErrorCode::ConstraintSeeds.
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(2006));

    Ok(())
}

/// A disabled operator must still be able to cancel their own airdrop.
///
/// Rationale: without this escape hatch the admin's disable / cap-lower
/// powers become a way to permanently strand operator rent (Bug 2 + Bug 4
/// interaction). `cancel_airdrop` deliberately does not consult
/// `is_enabled`, and this test pins that decision.
#[tokio::test]
async fn test_disabled_operator_can_cancel() -> eyre::Result<()> {
    let _serial = serial_lock().lock().await;
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("test_disabled_operator_can_cancel");
    let _enter = span.enter();

    let admin = &deployment.client;
    let store = deployment.store;
    let operator_client = deployment.user_client(Deployment::DEFAULT_USER)?;
    let operator = operator_client.payer();

    let airdrop_config = airdrop_config_pda(&store);

    // Enable the operator long enough to create an airdrop.
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: true,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    let nonce: [u8; 8] = [0xc4, 0, 0, 0, 0, 0, 0, 0];
    let airdrop = airdrop_pda(&store, &operator, &nonce);

    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CreateAirdrop {
            nonce,
            duration: 60,
        })
        .anchor_accounts(gt_incentive::accounts::CreateAirdrop {
            operator,
            store,
            airdrop_config,
            airdrop,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    // Admin disables the operator after the airdrop is live.
    admin
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::UpdateAirdropOperator {
            operator,
            timelock_secs: 3,
            max_airdrop_amount: 1_000_000,
            is_enabled: false,
        })
        .anchor_accounts(gt_incentive::accounts::UpdateAirdropOperator {
            authority: admin.payer(),
            store,
            airdrop_config,
            store_program: gmsol_store::ID,
        })
        .send()
        .await?;

    // Cancel still succeeds — the instruction does not consult the config.
    operator_client
        .store_transaction()
        .program(gt_incentive::ID)
        .anchor_args(gt_incentive::instruction::CancelAirdrop {})
        .anchor_accounts(gt_incentive::accounts::CancelAirdrop {
            operator,
            store,
            airdrop,
        })
        .send()
        .await?;

    Ok(())
}
