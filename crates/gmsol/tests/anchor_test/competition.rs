use crate::anchor_test::setup::{current_deployment, Deployment};
use anchor_lang::solana_program::system_program;
use chrono::{Duration as ChronoDur, Utc};
use eyre::Result;
use gmsol_competition::{accounts, instruction, ID as COMP_ID};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};

#[tokio::test]
async fn competition_flow() -> Result<()> {
    // -------- setup --------
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let slot = client.rpc().get_slot().await?;
    let now = client
        .rpc()
        .get_block_time(slot)
        .await
        .unwrap_or_else(|_| Utc::now().timestamp());
    let end = now + 3600; // one hour window
    let store_program = COMP_ID; // placeholder until real store program is ready

    // PDA to hold competition data
    let competition_kp = Keypair::new();

    // -------- 1) initialize --------
    client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::InitializeCompetition {
            competition: competition_kp.pubkey(),
            authority: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::InitializeCompetition {
            start_time: now,
            end_time: end,
            store_program,
        })
        .signer(&competition_kp)
        .send()
        .await?;

    // -------- 2) repeat initialize should fail --------
    let res = client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::InitializeCompetition {
            competition: competition_kp.pubkey(),
            authority: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::InitializeCompetition {
            start_time: now,
            end_time: end,
            store_program,
        })
        .signer(&competition_kp)
        .send()
        .await;
    assert!(res.is_err(), "duplicate initialize should fail");

    // -------- 3) record_trade within window --------
    let trader = Keypair::new();
    let participant_pda = Pubkey::find_program_address(
        &[
            b"participant",
            competition_kp.pubkey().as_ref(),
            trader.pubkey().as_ref(),
        ],
        &COMP_ID,
    )
    .0;

    client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::RecordTrade {
            competition: competition_kp.pubkey(),
            participant: participant_pda,
            store_program,
            trader: trader.pubkey(),
            payer: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::RecordTrade { volume: 123 })
        .send()
        .await?;

    // -------- 4) record_trade after window should fail --------
    let past_comp_kp = Keypair::new();
    let past_start = (Utc::now() - ChronoDur::hours(2)).timestamp();
    let past_end = (Utc::now() - ChronoDur::hours(1)).timestamp();
    client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::InitializeCompetition {
            competition: past_comp_kp.pubkey(),
            authority: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::InitializeCompetition {
            start_time: past_start,
            end_time: past_end,
            store_program,
        })
        .signer(&past_comp_kp)
        .send()
        .await?;

    let res = client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::RecordTrade {
            competition: past_comp_kp.pubkey(),
            participant: participant_pda,
            store_program,
            trader: trader.pubkey(),
            payer: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::RecordTrade { volume: 1 })
        .send()
        .await;
    assert!(res.is_err(), "trade outside window should fail");

    Ok(())
}

// ---------------- additional negative‑path tests ----------------

/// store_program ≠ competition.store_program  → should return InvalidCaller
#[tokio::test]
async fn competition_invalid_caller() -> Result<()> {
    let deployment = current_deployment().await?;
    let _g = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let now = Utc::now().timestamp();
    let end = now + 3600;
    let legit_store = COMP_ID;
    let bogus_store = Pubkey::new_unique();

    let comp_kp = Keypair::new();

    // init with legit store_program
    client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::InitializeCompetition {
            competition: comp_kp.pubkey(),
            authority: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::InitializeCompetition {
            start_time: now,
            end_time: end,
            store_program: legit_store,
        })
        .signer(&comp_kp)
        .send()
        .await?;

    // attempt record_trade from bogus store_program
    let trader = Keypair::new();
    let participant_pda = Pubkey::find_program_address(
        &[
            b"participant",
            comp_kp.pubkey().as_ref(),
            trader.pubkey().as_ref(),
        ],
        &COMP_ID,
    )
    .0;

    let res = client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::RecordTrade {
            competition: comp_kp.pubkey(),
            participant: participant_pda,
            store_program: bogus_store,
            trader: trader.pubkey(),
            payer: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::RecordTrade { volume: 1 })
        .send()
        .await;
    assert!(res.is_err(), "record_trade with invalid caller should fail");
    Ok(())
}

/// current < start_time  → should return OutsideCompetitionTime
#[tokio::test]
async fn competition_before_start() -> Result<()> {
    let deployment = current_deployment().await?;
    let _g = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let now = Utc::now().timestamp();
    let start = now + 600; // 10 min later
    let end = start + 600;

    let comp_kp = Keypair::new();
    client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::InitializeCompetition {
            competition: comp_kp.pubkey(),
            authority: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::InitializeCompetition {
            start_time: start,
            end_time: end,
            store_program: COMP_ID,
        })
        .signer(&comp_kp)
        .send()
        .await?;

    let trader = Keypair::new();
    let participant_pda = Pubkey::find_program_address(
        &[
            b"participant",
            comp_kp.pubkey().as_ref(),
            trader.pubkey().as_ref(),
        ],
        &COMP_ID,
    )
    .0;

    let res = client
        .store_transaction()
        .program(COMP_ID)
        .anchor_accounts(accounts::RecordTrade {
            competition: comp_kp.pubkey(),
            participant: participant_pda,
            store_program: COMP_ID,
            trader: trader.pubkey(),
            payer: client.payer(),
            system_program: system_program::ID,
        })
        .anchor_args(instruction::RecordTrade { volume: 42 })
        .send()
        .await;
    assert!(res.is_err(), "trade before start_time should fail");
    Ok(())
}
