use crate::anchor_test::setup::{current_deployment, Deployment};
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_sdk::client::ops::SystemProgramOps;
use gmsol_solana_utils::signer::shared_signer;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, InstructionError},
    signature::{Keypair, Signer},
    system_program,
    transaction::TransactionError,
};

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
    let evm_address = [0x23; 20];
    let (proxy_address, bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &evm_address],
        &evm_proxy::ID,
    );

    client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: client.payer(),
            proxy: proxy_address,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    let account = client.rpc().get_account(&proxy_address).await?;
    assert_eq!(account.owner, evm_proxy::ID);
    assert_eq!(
        account.data.len(),
        8 + std::mem::size_of::<evm_proxy::Proxy>()
    );
    assert_eq!(&account.data[..8], evm_proxy::Proxy::DISCRIMINATOR);
    let proxy = evm_proxy::Proxy::try_deserialize(&mut account.data.as_slice())?;
    assert_eq!(proxy.version, evm_proxy::PROXY_VERSION);
    assert_eq!(proxy.bump, bump);
    assert_eq!(proxy.evm_address, evm_address);
    assert_eq!(proxy.reserved, [0; 256]);
    Ok(())
}

#[tokio::test]
async fn proxy_account_lifecycle_rejects_invalid_creation() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let first_payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    let second_payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    for payer in [first_payer.payer(), second_payer.payer()] {
        deployment
            .client
            .transfer(&payer, 50_000_000)?
            .send()
            .await?;
    }

    let first_address = [0x34; 20];
    let second_address = [0x56; 20];
    let (first_proxy, first_bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &first_address],
        &evm_proxy::ID,
    );
    let (second_proxy, second_bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &second_address],
        &evm_proxy::ID,
    );
    assert_ne!(first_proxy, second_proxy);
    assert!(first_payer
        .rpc()
        .get_account_with_commitment(&first_proxy, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    first_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: first_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let original = first_payer.rpc().get_account(&first_proxy).await?;
    let decoded = evm_proxy::Proxy::try_deserialize(&mut original.data.as_slice())?;
    assert_eq!(decoded.version, evm_proxy::PROXY_VERSION);
    assert_eq!(decoded.bump, first_bump);
    assert_eq!(decoded.evm_address, first_address);

    second_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        original
    );

    let forged_proxy = solana_sdk::pubkey::Pubkey::new_unique();
    let error = second_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: second_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: forged_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a forged proxy address must fail seed validation");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert!(second_payer
        .rpc()
        .get_account_with_commitment(&forged_proxy, CommitmentConfig::processed())
        .await?
        .value
        .is_none());
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        original
    );

    let error = second_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: second_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("an address cannot reuse another address's proxy");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        original
    );
    assert!(second_payer
        .rpc()
        .get_account_with_commitment(&second_proxy, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    second_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: second_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: second_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let second = second_payer.rpc().get_account(&second_proxy).await?;
    let decoded = evm_proxy::Proxy::try_deserialize(&mut second.data.as_slice())?;
    assert_eq!(decoded.evm_address, second_address);
    assert_eq!(decoded.bump, second_bump);
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        original
    );

    let rent_payer = Keypair::new();
    let empty_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(0)
        .await?;
    let proxy_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(8 + std::mem::size_of::<evm_proxy::Proxy>())
        .await?;
    assert!(empty_rent < proxy_rent);
    deployment
        .client
        .transfer(&rent_payer.pubkey(), empty_rent)?
        .send()
        .await?;
    let rent_address = [0x78; 20];
    let (rent_proxy, _) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &rent_address],
        &evm_proxy::ID,
    );
    let error = first_payer
        .store_transaction()
        .program(evm_proxy::ID)
        .signer(&rent_payer)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: rent_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: rent_payer.pubkey(),
            proxy: rent_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a signer with less than proxy rent cannot create an account");
    let transaction_error = match &error {
        gmsol_solana_utils::Error::Client(client_error) => client_error.get_transaction_error(),
        _ => panic!("unexpected insufficient-rent error: {error:?}"),
    };
    assert!(
        matches!(
            transaction_error,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::Custom(1)
            ))
        ),
        "unexpected insufficient-rent error: {error:?}"
    );
    assert_eq!(
        first_payer.rpc().get_balance(&rent_payer.pubkey()).await?,
        empty_rent
    );
    assert!(first_payer
        .rpc()
        .get_account_with_commitment(&rent_proxy, CommitmentConfig::processed())
        .await?
        .value
        .is_none());
    Ok(())
}

#[tokio::test]
async fn prefunded_proxy_is_created_and_scanned_by_address() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let evm_address = [0x9a; 20];
    let (proxy_address, bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &evm_address],
        &evm_proxy::ID,
    );
    client.transfer(&proxy_address, 1_000_000)?.send().await?;
    let before = client.rpc().get_account(&proxy_address).await?;
    assert_eq!(before.owner, system_program::ID);
    assert!(before.data.is_empty());

    client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: client.payer(),
            proxy: proxy_address,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let after = client.rpc().get_account(&proxy_address).await?;
    assert_eq!(after.owner, evm_proxy::ID);
    assert_eq!(after.data.len(), 296);
    assert!(after.lamports >= before.lamports);

    let accounts = client.rpc().get_program_accounts(&evm_proxy::ID).await?;
    let proxies: Vec<_> = accounts
        .into_iter()
        .filter(|(_, account)| account.data.starts_with(evm_proxy::Proxy::DISCRIMINATOR))
        .collect();
    assert!(proxies.iter().any(|(key, _)| *key == proxy_address));
    for (key, account) in proxies {
        assert_eq!(account.owner, evm_proxy::ID);
        assert_eq!(account.data.len(), 296);
        let state = evm_proxy::Proxy::try_deserialize(&mut account.data.as_slice())?;
        let (derived, derived_bump) = solana_sdk::pubkey::Pubkey::find_program_address(
            &[evm_proxy::PROXY_SEED, &state.evm_address],
            &evm_proxy::ID,
        );
        assert_eq!(key, derived);
        assert_eq!(state.bump, derived_bump);
        assert_eq!(state.version, evm_proxy::PROXY_VERSION);
    }
    let state = evm_proxy::Proxy::try_deserialize(&mut after.data.as_slice())?;
    assert_eq!(state.evm_address, evm_address);
    assert_eq!(state.bump, bump);
    Ok(())
}

#[tokio::test]
async fn proxy_has_no_writable_close_entrypoint() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let evm_address = [0xbc; 20];
    let (proxy_address, _) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &evm_address],
        &evm_proxy::ID,
    );
    client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: client.payer(),
            proxy: proxy_address,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let before = client.rpc().get_account(&proxy_address).await?;
    let error = client
        .store_transaction()
        .program(evm_proxy::ID)
        .args(vec![0xff; 8])
        .accounts(vec![AccountMeta::new(proxy_address, false)])
        .send()
        .await
        .expect_err("an unknown writable-proxy instruction must fail");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::InstructionFallbackNotFound.into())
    );
    assert_eq!(client.rpc().get_account(&proxy_address).await?, before);
    Ok(())
}

#[test]
fn proxy_future_reserved_bytes_preserve_identity_and_abi() -> eyre::Result<()> {
    let evm_address = [0xde; 20];
    let (proxy_address, bump) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &evm_address],
        &evm_proxy::ID,
    );
    let mut data = vec![0; 296];
    data[..8].copy_from_slice(evm_proxy::Proxy::DISCRIMINATOR);
    data[8] = evm_proxy::PROXY_VERSION;
    data[9] = bump;
    data[16..36].copy_from_slice(&evm_address);
    data[40..48].copy_from_slice(&42_u64.to_le_bytes());
    let state = evm_proxy::Proxy::try_deserialize(&mut data.as_slice())?;
    assert_eq!(state.version, evm_proxy::PROXY_VERSION);
    assert_eq!(state.bump, bump);
    assert_eq!(state.evm_address, evm_address);
    assert_eq!(&state.reserved[..8], &42_u64.to_le_bytes());
    assert_eq!(data.len(), 296);
    let (derived, _) = solana_sdk::pubkey::Pubkey::find_program_address(
        &[evm_proxy::PROXY_SEED, &state.evm_address],
        &evm_proxy::ID,
    );
    assert_eq!(derived, proxy_address);

    let idl: serde_json::Value = serde_json::from_str(include_str!(
        "../../crates/programs/idls/gmsol_evm_proxy.json"
    ))?;
    let names: Vec<_> = idl["instructions"]
        .as_array()
        .expect("IDL instructions")
        .iter()
        .map(|instruction| instruction["name"].as_str().expect("instruction name"))
        .collect();
    assert_eq!(names, ["create_proxy", "initialize"]);
    Ok(())
}
