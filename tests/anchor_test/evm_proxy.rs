use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_evm_proxy as evm_proxy;

#[tokio::test]
async fn evm_proxy_initializes_through_anchor() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let instruction = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::Initialize {})
        .anchor_accounts(evm_proxy::accounts::Initialize {
            authority: client.payer(),
            system_program: solana_sdk::system_program::ID,
        });
    instruction.send().await?;

    Ok(())
}
