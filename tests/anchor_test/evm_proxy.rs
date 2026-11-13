use crate::anchor_test::setup::{current_deployment, Deployment};
use base64::Engine;
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{
    error::ErrorCode, AccountDeserialize, AnchorDeserialize, Discriminator,
};
use gmsol_sdk::client::ops::evm_proxy::ConfigInitialization;
use gmsol_sdk::client::ops::SystemProgramOps;
use gmsol_solana_utils::signer::shared_signer;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, InstructionError},
    signature::{EncodableKey, Keypair, Signer},
    system_program,
    transaction::TransactionError,
};
use solana_transaction_status::{option_serializer::OptionSerializer, UiTransactionEncoding};

#[tokio::test]
async fn config_creation_binds_store_and_emits_event() -> eyre::Result<()> {
    use solana_client::rpc_config::RpcTransactionConfig;

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let client = &deployment.client;
    let program_data = bpf_loader_upgradeable::get_program_data_address(&evm_proxy::ID);
    let program_data_account = client.rpc().get_account(&program_data).await?;
    assert_eq!(program_data_account.owner, bpf_loader_upgradeable::ID);
    let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
    let state: UpgradeableLoaderState =
        bincode::deserialize(&program_data_account.data[..metadata_len])?;
    assert!(matches!(
        state,
        UpgradeableLoaderState::ProgramData {
            upgrade_authority_address: Some(authority),
            ..
        } if authority == client.payer()
    ));

    let (config_address, _) =
        solana_sdk::pubkey::Pubkey::find_program_address(&[evm_proxy::CONFIG_SEED], &evm_proxy::ID);
    let genesis_hash = client.rpc().get_genesis_hash().await?.to_bytes();
    let admin = client.payer();
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let wrong_authority = other_client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: admin,
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: other_client.payer(),
            upgrade_authority: other_client.payer(),
            config: config_address,
            program: evm_proxy::ID,
            program_data,
            store: deployment.store,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("the keeper is not the program upgrade authority");
    assert_eq!(
        gmsol_sdk::Error::from(wrong_authority).anchor_error_code(),
        Some(ErrorCode::ConstraintRaw.into())
    );
    assert!(client
        .rpc()
        .get_account_with_commitment(&config_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let zero_admin = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: solana_sdk::pubkey::Pubkey::default(),
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: client.payer(),
            upgrade_authority: client.payer(),
            config: config_address,
            program: evm_proxy::ID,
            program_data,
            store: deployment.store,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a zero first admin must not initialize Config");
    assert_eq!(
        gmsol_sdk::Error::from(zero_admin).anchor_error_code(),
        Some(6002)
    );
    assert!(client
        .rpc()
        .get_account_with_commitment(&config_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let foreign_store = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: admin,
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: client.payer(),
            upgrade_authority: client.payer(),
            config: config_address,
            program: evm_proxy::ID,
            program_data,
            store: system_program::ID,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a foreign-owned account cannot be bound as Store");
    assert_eq!(
        gmsol_sdk::Error::from(foreign_store).anchor_error_code(),
        Some(ErrorCode::AccountOwnedByWrongProgram.into())
    );
    assert!(client
        .rpc()
        .get_account_with_commitment(&config_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let wrong_program_data = bpf_loader_upgradeable::get_program_data_address(&gmsol_store::ID);
    let unrelated = client.rpc().get_account(&wrong_program_data).await?;
    assert_eq!(unrelated.owner, bpf_loader_upgradeable::ID);
    let error = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: admin,
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: client.payer(),
            upgrade_authority: client.payer(),
            config: config_address,
            program: evm_proxy::ID,
            program_data: wrong_program_data,
            store: deployment.store,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("another program's ProgramData must not authorize Config");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::ConstraintRaw.into())
    );
    assert!(client
        .rpc()
        .get_account_with_commitment(&config_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let forged_config = solana_sdk::pubkey::Pubkey::new_unique();
    let error = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: admin,
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: client.payer(),
            upgrade_authority: client.payer(),
            config: forged_config,
            program: evm_proxy::ID,
            program_data,
            store: deployment.store,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a noncanonical Config address must fail seed validation");
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert!(client
        .rpc()
        .get_account_with_commitment(&forged_config, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let mut wrong_genesis_hash = genesis_hash;
    wrong_genesis_hash[0] ^= 1;
    let wallet = std::env::var("ANCHOR_WALLET")?;
    let upgrade_authority = Keypair::read_from_file(&wallet)
        .map_err(|error| eyre::eyre!("upgrade authority wallet unavailable: {error}"))?;
    assert_eq!(upgrade_authority.pubkey(), admin);
    let mismatch = client
        .initialize_proxy_config(
            deployment.store,
            wrong_genesis_hash,
            admin,
            &upgrade_authority,
        )
        .await
        .expect_err("a mismatched RPC genesis hash must reject before send");
    assert!(matches!(
        mismatch,
        gmsol_sdk::Error::Custom(ref message)
            if message == "supplied genesis hash differs from RPC"
    ));
    assert!(client
        .rpc()
        .get_account_with_commitment(&config_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());

    let signature = match client
        .initialize_proxy_config(deployment.store, genesis_hash, admin, &upgrade_authority)
        .await?
    {
        ConfigInitialization::Verified {
            signature,
            current_admin,
        } => {
            assert_eq!(current_admin, admin);
            signature
        }
        ConfigInitialization::VerificationIncomplete { reason, .. } => {
            eyre::bail!("confirmed Config creation failed readback: {reason}")
        }
    };

    let account = client.rpc().get_account(&config_address).await?;
    assert_eq!(account.owner, evm_proxy::ID);
    assert_eq!(&account.data[..8], evm_proxy::Config::DISCRIMINATOR);
    let config = evm_proxy::Config::try_deserialize(&mut account.data.as_slice())?;
    assert_eq!(config.store, deployment.store);
    assert_eq!(config.genesis_hash, genesis_hash);
    assert_eq!(config.admin, admin);
    assert_eq!(config.pending_admin, None);
    assert!(config.paused);
    assert!(config.allowed_chain_ids.is_empty());

    let duplicate_error = client
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateConfig {
            genesis_hash,
            first_admin: admin,
        })
        .anchor_accounts(evm_proxy::accounts::CreateConfig {
            payer: client.payer(),
            upgrade_authority: client.payer(),
            config: config_address,
            program: evm_proxy::ID,
            program_data,
            store: deployment.store,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a second Config initialization must fail");
    let duplicate_transaction_error = match &duplicate_error {
        gmsol_solana_utils::Error::Client(error) => error.get_transaction_error(),
        _ => panic!("unexpected duplicate initialization error: {duplicate_error:?}"),
    };
    assert!(
        matches!(
            duplicate_transaction_error,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::Custom(0)
            ))
        ),
        "unexpected duplicate initialization error: {duplicate_error:?}"
    );
    assert_eq!(client.rpc().get_account(&config_address).await?, account);

    let transaction = client
        .rpc()
        .get_transaction_with_config(
            &signature,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        )
        .await?;
    let logs = match transaction
        .transaction
        .meta
        .expect("transaction metadata")
        .log_messages
    {
        OptionSerializer::Some(logs) => logs,
        other => panic!("transaction logs unavailable: {other:?}"),
    };
    let events: Vec<_> = logs
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .filter_map(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .filter(|data| data.starts_with(evm_proxy::ConfigCreated::DISCRIMINATOR))
        .map(|data| evm_proxy::ConfigCreated::deserialize(&mut &data[8..]))
        .collect::<Result<_, _>>()?;
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.config, config_address);
    assert_eq!(event.store, deployment.store);
    assert_eq!(event.genesis_hash, genesis_hash);
    assert_eq!(event.admin, admin);
    assert!(event.paused);
    assert!(event.allowed_chain_ids.is_empty());
    super::cancel::verify_signed_cancel(&deployment, config_address, genesis_hash).await?;
    super::referral::verify_signed_referral(&deployment, genesis_hash).await?;
    super::config_policy::verify_policy_and_handover(&deployment, config_address, genesis_hash)
        .await?;
    Ok(())
}

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
    assert_eq!(
        names,
        [
            "accept_admin",
            "cancel",
            "create_config",
            "create_proxy",
            "initialize",
            "propose_admin",
            "set_chain_allowed",
            "set_paused",
            "set_referral_code",
            "withdraw",
        ]
    );
    Ok(())
}
