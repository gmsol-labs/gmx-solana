use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_sdk::evm_proxy::ProxyCPIEvent;
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
    assert_eq!(proxy.next_nonce, 0);
    let sdk_proxy = gmsol_programs::gmsol_evm_proxy::accounts::Proxy::try_deserialize(
        &mut account.data.as_slice(),
    )?;
    assert_eq!(sdk_proxy.next_nonce, 0);
    assert_eq!(sdk_proxy.evm_address, evm_address);
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

#[tokio::test]
async fn config_creation_records_immutable_policy() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = super::setup::EVM_PROXY_CONFIG_LOCK.lock().await;
    let client = &deployment.client;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let state = super::config_policy::assert_baseline(&deployment).await?;
    let history = client.rpc().get_signatures_for_address(&config).await?;
    let signature = history
        .iter()
        .rev()
        .find(|transaction| transaction.err.is_none())
        .ok_or_else(|| eyre::eyre!("Config creation history missing"))?
        .signature
        .parse()?;
    let ProxyCPIEvent::ConfigCreated(event) =
        super::config_policy::event(client, &signature).await?
    else {
        eyre::bail!("expected ConfigCreated CPI event");
    };
    assert_eq!(event.config, config);
    assert_eq!(event.store, deployment.store);
    assert_eq!(event.genesis_hash, state.genesis_hash);
    assert_eq!(event.admin, client.payer());
    assert!(event.paused);
    assert!(event.allowed_chain_ids.is_empty());
    Ok(())
}

#[tokio::test]
async fn config_cannot_be_initialized_twice() -> eyre::Result<()> {
    use gmsol_sdk::client::ops::{evm_proxy::create_config_instruction, EvmProxyOps};
    use solana_sdk::{
        instruction::InstructionError, system_instruction::SystemError,
        transaction::TransactionError,
    };

    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = super::setup::EVM_PROXY_CONFIG_LOCK.lock().await;
    let client = &deployment.client;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let (before, state) = super::config_policy::read_config(client, &config).await?;
    let error = client
        .evm_proxy_instruction(create_config_instruction(
            evm_proxy::ID,
            client.payer(),
            client.payer(),
            deployment.store,
            state.genesis_hash,
            client.payer(),
        ))
        .send()
        .await
        .expect_err("one-time Config initialization");
    let transaction_error = match &error {
        gmsol_solana_utils::Error::Client(error) => error.get_transaction_error(),
        _ => panic!("unexpected duplicate initialization error: {error:?}"),
    };
    assert!(
        matches!(transaction_error, Some(TransactionError::InstructionError(_, InstructionError::Custom(code))) if code == SystemError::AccountAlreadyInUse as u32)
    );
    assert_eq!(client.rpc().get_account(&config).await?, before);
    Ok(())
}
