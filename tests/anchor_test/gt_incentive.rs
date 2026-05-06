use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_gt_incentive as gt_incentive;

#[tokio::test]
async fn test_gt_incentive() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;

    // Placeholder test
    _ = gt_incentive::accounts::Initialize {
        authority: client.payer(),
        system_program: solana_sdk::system_program::ID,
    };

    Ok(())
}
