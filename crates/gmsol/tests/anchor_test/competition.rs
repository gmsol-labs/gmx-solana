use gmsol_competition::{accounts, instruction};

use crate::anchor_test::setup::{current_deployment, Deployment};

#[tokio::test]
async fn competition() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let span = tracing::info_span!("competition");
    let _enter = span.enter();

    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    let signature = client
        .store_transaction()
        .program(gmsol_competition::ID)
        .anchor_accounts(accounts::Initialize {})
        .anchor_args(instruction::Initialize {})
        .send()
        .await?;

    tracing::info!("initialized: {signature}");

    Ok(())
}
