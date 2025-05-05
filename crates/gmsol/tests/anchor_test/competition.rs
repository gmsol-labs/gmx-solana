use chrono::Utc;
use solana_sdk::signature::{Keypair, Signer};
use anchor_lang::solana_program::system_program;
use gmsol_competition::{accounts, instruction};
use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn competition() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard     = deployment.use_accounts().await?;
    let client     = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let now  = Utc::now().timestamp();
    let end  = now + 3600;                        
    let store_program = gmsol_competition::ID;

    let competition_kp = Keypair::new();

    let sig = client
    .store_transaction()
    .program(gmsol_competition::ID)
    .anchor_accounts(accounts::InitializeCompetition {
        competition: competition_kp.pubkey(),
        authority:   client.payer(),
        system_program: system_program::ID,
    })
    .anchor_args(instruction::InitializeCompetition {
        start_time: now,
        end_time:   end,
        store_program,
    })
    .signer(&competition_kp)
    .send()
    .await?;
    tracing::info!("initialized: {sig}");

    Ok(())
}