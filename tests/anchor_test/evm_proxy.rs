use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_utils::InitSpace;
use solana_sdk::{commitment_config::CommitmentConfig, system_program};

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

#[tokio::test]
async fn proxy_account_creation_records_evm_address() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let evm_address = [0x23; evm_proxy::EVM_ADDRESS_LEN];
    let (proxy_address, bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &evm_address],
        &evm_proxy::ID,
    );

    client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
            payer: client.payer(),
            proxy: proxy_address,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    let account = client.rpc().get_account(&proxy_address).await?;
    assert_eq!(account.owner, evm_proxy::ID);
    assert_eq!(account.data.len(), 8 + evm_proxy::Proxy::INIT_SPACE);
    assert!(account.data.starts_with(evm_proxy::Proxy::DISCRIMINATOR));
    let proxy = evm_proxy::Proxy::try_deserialize(&mut account.data.as_slice())?;
    assert_eq!(proxy.version, evm_proxy::PROXY_VERSION);
    assert_eq!(proxy.bump, bump);
    assert_eq!(proxy.evm_address, evm_address);
    assert_eq!(proxy.reserved, [0; evm_proxy::PROXY_RESERVED_LEN]);
    Ok(())
}

#[tokio::test]
async fn proxy_second_payer_preserves_existing_account() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let first_payer = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let second_payer = &deployment.client;
    assert_ne!(first_payer.payer(), second_payer.payer());
    let evm_address = [0x34; evm_proxy::EVM_ADDRESS_LEN];
    let proxy = gmsol_sdk::pda::find_proxy_address(&evm_address, &evm_proxy::ID).0;
    first_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
            payer: first_payer.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let before = first_payer.rpc().get_account(&proxy).await?;
    second_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
            payer: second_payer.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    assert_eq!(second_payer.rpc().get_account(&proxy).await?, before);
    Ok(())
}

#[tokio::test]
async fn proxy_creation_rejects_mismatched_address() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let evm_address = [0x56; evm_proxy::EVM_ADDRESS_LEN];
    let mismatched_address = [0x78; evm_proxy::EVM_ADDRESS_LEN];
    let proxy = gmsol_sdk::pda::find_proxy_address(&evm_address, &evm_proxy::ID).0;
    let mismatched_proxy =
        gmsol_sdk::pda::find_proxy_address(&mismatched_address, &evm_proxy::ID).0;
    let error = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxyIdempotent {
            evm_address: mismatched_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
            payer: client.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a mismatched EVM address must fail seed validation");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    for address in [proxy, mismatched_proxy] {
        assert!(client
            .rpc()
            .get_account_with_commitment(&address, CommitmentConfig::processed())
            .await?
            .value
            .is_none());
    }
    Ok(())
}
