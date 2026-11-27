use crate::anchor_test::setup::{current_deployment, Deployment};
use eyre::OptionExt;
use futures_util::{stream, StreamExt};
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_sdk::{
    client::{
        ops::{evm_proxy::create_config_instruction, EvmProxyOps, SystemProgramOps},
        transaction_history::extract_cpi_events,
    },
    decode::{gmsol::programs::evm_proxy::EvmProxyCPIEvent, Decode},
    Client,
};
use gmsol_solana_utils::{
    signer::{shared_signer, SignerRef},
    transaction_builder::{default_before_sign, TransactionBuilder},
    utils::WithSlot,
};
use gmsol_utils::InitSpace;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_program,
};

type PolicyClient = Client<SignerRef>;

const NOMINEE_FUNDING_LAMPORTS: u64 = 10_000_000;

async fn read_config(
    client: &PolicyClient,
    address: &Pubkey,
) -> eyre::Result<(Account, evm_proxy::Config)> {
    let account = client.rpc().get_account(address).await?;
    let state = evm_proxy::Config::try_deserialize(&mut account.data.as_slice())?;
    Ok((account, state))
}

async fn send(transaction: TransactionBuilder<'_, SignerRef>) -> eyre::Result<WithSlot<Signature>> {
    Ok(transaction
        .send_with_options(false, None, None, Default::default(), default_before_sign)
        .await?)
}

async fn event(
    client: &PolicyClient,
    signature: WithSlot<Signature>,
) -> eyre::Result<EvmProxyCPIEvent> {
    let program = client.evm_proxy_program_for_builders();
    let events = extract_cpi_events(
        stream::iter([Ok::<_, gmsol_sdk::Error>(signature)]),
        client.rpc(),
        &program.id,
        &program.find_event_authority_address(),
        CommitmentConfig::confirmed(),
        Some(0),
    );
    futures_util::pin_mut!(events);
    let transaction = events
        .next()
        .await
        .ok_or_eyre("no events in the proxy transaction")??;
    let mut events = transaction
        .value()
        .events
        .iter()
        .map(EvmProxyCPIEvent::decode)
        .collect::<Result<Vec<_>, _>>()?;
    eyre::ensure!(
        events.len() == 1,
        "expected one proxy CPI event, got {}",
        events.len()
    );
    Ok(events.remove(0))
}

fn assert_anchor_error(error: gmsol_solana_utils::Error, expected: u32) {
    assert_eq!(
        gmsol_sdk::Error::from(error).anchor_error_code(),
        Some(expected)
    );
}

fn chain_id(high: u8, low: u8) -> [u8; 32] {
    let mut id = [0; 32];
    id[0] = high;
    id[id.len() - 1] = low;
    id
}

async fn funded_nominee(client: &PolicyClient) -> eyre::Result<PolicyClient> {
    let nominee = Keypair::new();
    client
        .transfer(&nominee.pubkey(), NOMINEE_FUNDING_LAMPORTS)?
        .send()
        .await?;
    Ok(client.try_clone_with_payer(shared_signer(nominee))?)
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
    assert_eq!(proxy.reserved, [0; evm_proxy::PROXY_RESERVED_LEN]);
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
    let client = &deployment.client;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    deployment
        .with_evm_proxy_config(&[], async {
            let (_, state) = read_config(client, &config).await?;
            let history = client
                .rpc()
                .get_signatures_for_address_with_config(
                    &config,
                    GetConfirmedSignaturesForAddress2Config {
                        commitment: Some(CommitmentConfig::confirmed()),
                        ..Default::default()
                    },
                )
                .await?;
            let creation = history
                .iter()
                .rev()
                .find(|transaction| transaction.err.is_none())
                .ok_or_eyre("Config creation history missing")?;
            let signature = WithSlot::new(creation.slot, creation.signature.parse()?);
            let EvmProxyCPIEvent::ConfigCreated(event) = event(client, signature).await? else {
                eyre::bail!("expected ConfigCreated CPI event");
            };
            assert_eq!(event.config, config);
            assert_eq!(event.store, deployment.store);
            assert_eq!(event.genesis_hash, state.genesis_hash);
            assert_eq!(event.admin, client.payer());
            assert!(event.paused);
            assert!(event.allowed_chain_ids.is_empty());
            Ok(())
        })
        .await
}

#[tokio::test]
async fn config_cannot_be_initialized_twice() -> eyre::Result<()> {
    use solana_sdk::{
        instruction::InstructionError, system_instruction::SystemError,
        transaction::TransactionError,
    };

    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    deployment
        .with_evm_proxy_config(&[], async {
            let (before, state) = read_config(client, &config).await?;
            let error = client
                .store_transaction()
                .pre_instruction(
                    create_config_instruction(
                        client.evm_proxy_program_for_builders(),
                        client.payer(),
                        client.payer(),
                        deployment.store,
                        state.genesis_hash,
                        client.payer(),
                    ),
                    false,
                )
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
        })
        .await
}

#[tokio::test]
async fn allowlist_enforces_authority_capacity_and_noops() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    deployment
        .with_evm_proxy_config(&[], async {
            let first = chain_id(0x80, 1);
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                other_client
                    .set_proxy_chain_allowed(first, true)
                    .send()
                    .await
                    .expect_err("non-admin chain change"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            let signature = send(client.set_proxy_chain_allowed(first, true)).await?;
            let EvmProxyCPIEvent::ConfigChainAllowedChanged(changed) =
                event(client, signature).await?
            else {
                eyre::bail!("expected ConfigChainAllowedChanged CPI event");
            };
            assert_eq!(
                (changed.config, changed.chain_id, changed.allowed),
                (config, first, true)
            );
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(state.allowed_chain_ids, vec![first]);
            assert_anchor_error(
                client
                    .set_proxy_chain_allowed(first, true)
                    .send()
                    .await
                    .expect_err("duplicate chain add"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            let capacity = u8::try_from(evm_proxy::MAX_ALLOWED_CHAINS)?;
            for low in 2..=capacity {
                client
                    .set_proxy_chain_allowed(chain_id(0x80, low), true)
                    .send()
                    .await?;
            }
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(state.allowed_chain_ids.len(), evm_proxy::MAX_ALLOWED_CHAINS);
            assert_eq!(state.allowed_chain_ids[0], first);
            assert_eq!(
                state.allowed_chain_ids[evm_proxy::MAX_ALLOWED_CHAINS - 1],
                chain_id(0x80, capacity)
            );
            assert_anchor_error(
                client
                    .set_proxy_chain_allowed(chain_id(0x80, capacity + 1), true)
                    .send()
                    .await
                    .expect_err("chain capacity"),
                evm_proxy::ProxyError::AllowedChainsFull.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            let signature = send(client.set_proxy_chain_allowed(first, false)).await?;
            let EvmProxyCPIEvent::ConfigChainAllowedChanged(removed) =
                event(client, signature).await?
            else {
                eyre::bail!("expected ConfigChainAllowedChanged CPI event");
            };
            assert_eq!(
                (removed.config, removed.chain_id, removed.allowed),
                (config, first, false)
            );
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(
                state.allowed_chain_ids.len(),
                evm_proxy::MAX_ALLOWED_CHAINS - 1
            );
            assert_anchor_error(
                client
                    .set_proxy_chain_allowed(first, false)
                    .send()
                    .await
                    .expect_err("duplicate chain removal"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            Ok(())
        })
        .await
}

#[tokio::test]
async fn pause_changes_require_admin_and_reject_noops() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    deployment
        .with_evm_proxy_config(&[], async {
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                other_client
                    .set_proxy_paused(false)
                    .send()
                    .await
                    .expect_err("non-admin pause change"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            for (previous, paused) in [(true, false), (false, true)] {
                let signature = send(client.set_proxy_paused(paused)).await?;
                let EvmProxyCPIEvent::ConfigPauseChanged(changed) =
                    event(client, signature).await?
                else {
                    eyre::bail!("expected ConfigPauseChanged CPI event");
                };
                assert_eq!(
                    (changed.config, changed.previous_paused, changed.paused),
                    (config, previous, paused)
                );
                assert_eq!(read_config(client, &config).await?.1.paused, paused);
            }
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                client
                    .set_proxy_paused(true)
                    .send()
                    .await
                    .expect_err("pause no-op"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            Ok(())
        })
        .await
}

#[tokio::test]
async fn handover_requires_latest_nominee_and_revokes_previous_admin() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let first_client = funded_nominee(client).await?;
    let first_nominee_address = first_client.payer();
    let second_client = funded_nominee(client).await?;
    let second_nominee_address = second_client.payer();
    deployment
        .with_evm_proxy_config(&[&first_client, &second_client], async {
            let genesis_hash = read_config(client, &config).await?.1.genesis_hash;
            let signature = send(client.transfer_proxy_admin(first_nominee_address)).await?;
            let EvmProxyCPIEvent::ConfigAdminProposed(proposal) = event(client, signature).await?
            else {
                eyre::bail!("expected ConfigAdminProposed CPI event");
            };
            assert_eq!(proposal.config, config);
            assert_eq!(proposal.admin, client.payer());
            assert_eq!(proposal.previous_next_admin, None);
            assert_eq!(proposal.next_admin, first_nominee_address);
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(state.admin, client.payer());
            assert_eq!(state.next_admin, Some(first_nominee_address));
            assert_anchor_error(
                client
                    .transfer_proxy_admin(first_nominee_address)
                    .send()
                    .await
                    .expect_err("same nomination"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_anchor_error(
                client
                    .transfer_proxy_admin(Pubkey::default())
                    .send()
                    .await
                    .expect_err("zero nominee"),
                evm_proxy::ProxyError::InvalidAdmin.into(),
            );
            assert_anchor_error(
                first_client
                    .set_proxy_paused(false)
                    .send()
                    .await
                    .expect_err("pending admin has no authority"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            let signature = send(client.set_proxy_paused(false)).await?;
            let EvmProxyCPIEvent::ConfigPauseChanged(old_admin_change) =
                event(client, signature).await?
            else {
                eyre::bail!("expected ConfigPauseChanged CPI event");
            };
            assert_eq!(
                (old_admin_change.previous_paused, old_admin_change.paused),
                (true, false)
            );

            let signature = send(client.transfer_proxy_admin(second_nominee_address)).await?;
            let EvmProxyCPIEvent::ConfigAdminProposed(replacement) =
                event(client, signature).await?
            else {
                eyre::bail!("expected ConfigAdminProposed CPI event");
            };
            assert_eq!(replacement.previous_next_admin, Some(first_nominee_address));
            assert_eq!(replacement.next_admin, second_nominee_address);
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                first_client
                    .accept_proxy_admin()
                    .send()
                    .await
                    .expect_err("superseded nominee cannot accept"),
                evm_proxy::ProxyError::NotPendingAdmin.into(),
            );
            assert_anchor_error(
                other_client
                    .accept_proxy_admin()
                    .send()
                    .await
                    .expect_err("unrelated signer cannot accept"),
                evm_proxy::ProxyError::NotPendingAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            let signature = send(second_client.accept_proxy_admin()).await?;
            let EvmProxyCPIEvent::ConfigAdminAccepted(accepted) = event(client, signature).await?
            else {
                eyre::bail!("expected ConfigAdminAccepted CPI event");
            };
            assert_eq!(accepted.config, config);
            assert_eq!(accepted.previous_admin, client.payer());
            assert_eq!(accepted.admin, second_nominee_address);
            assert_eq!(accepted.next_admin, None);
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(state.admin, second_nominee_address);
            assert_eq!(state.next_admin, None);
            assert_eq!(state.store, deployment.store);
            assert_eq!(state.genesis_hash, genesis_hash);
            assert_anchor_error(
                client
                    .set_proxy_paused(true)
                    .send()
                    .await
                    .expect_err("former admin lost authority"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_anchor_error(
                second_client
                    .accept_proxy_admin()
                    .send()
                    .await
                    .expect_err("handover cannot be repeated"),
                evm_proxy::ProxyError::NotPendingAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            let signature = send(second_client.set_proxy_paused(true)).await?;
            let EvmProxyCPIEvent::ConfigPauseChanged(changed) = event(client, signature).await?
            else {
                eyre::bail!("expected ConfigPauseChanged CPI event");
            };
            assert_eq!((changed.previous_paused, changed.paused), (false, true));
            let (_, state) = read_config(client, &config).await?;
            assert_eq!(state.store, deployment.store);
            assert_eq!(state.genesis_hash, genesis_hash);
            assert_eq!(state.admin, second_nominee_address);
            assert!(state.paused);
            Ok(())
        })
        .await
}

#[tokio::test]
async fn admin_transfer_requires_current_admin() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let nominee = funded_nominee(client).await?;
    deployment
        .with_evm_proxy_config(&[&nominee], async {
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                other_client
                    .transfer_proxy_admin(other_client.payer())
                    .send()
                    .await
                    .expect_err("non-admin nomination"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            client.transfer_proxy_admin(nominee.payer()).send().await?;
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                nominee
                    .transfer_proxy_admin(other_client.payer())
                    .send()
                    .await
                    .expect_err("pending admin cannot nominate"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            nominee.accept_proxy_admin().send().await?;
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                client
                    .transfer_proxy_admin(other_client.payer())
                    .send()
                    .await
                    .expect_err("former admin cannot nominate"),
                evm_proxy::ProxyError::NotConfigAdmin.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            Ok(())
        })
        .await
}

#[tokio::test]
async fn admin_withdraws_pending_nomination() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let nominee = funded_nominee(client).await?;
    deployment
        .with_evm_proxy_config(&[], async {
            let (before, _) = read_config(client, &config).await?;
            assert_anchor_error(
                client
                    .transfer_proxy_admin(client.payer())
                    .send()
                    .await
                    .expect_err("no nomination to withdraw"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);

            client.transfer_proxy_admin(nominee.payer()).send().await?;
            let (before, _) = read_config(client, &config).await?;
            for (signer, reason) in [
                (&nominee, "pending admin cannot withdraw"),
                (&other_client, "non-admin cannot withdraw"),
            ] {
                assert_anchor_error(
                    signer
                        .transfer_proxy_admin(client.payer())
                        .send()
                        .await
                        .expect_err(reason),
                    evm_proxy::ProxyError::NotConfigAdmin.into(),
                );
            }
            assert_eq!(read_config(client, &config).await?.0, before);

            let signature = send(client.transfer_proxy_admin(client.payer())).await?;
            let EvmProxyCPIEvent::ConfigAdminProposalCancelled(cancelled) =
                event(client, signature).await?
            else {
                eyre::bail!("expected ConfigAdminProposalCancelled CPI event");
            };
            assert_eq!(
                (
                    cancelled.config,
                    cancelled.admin,
                    cancelled.previous_next_admin
                ),
                (config, client.payer(), nominee.payer())
            );
            let (before, state) = read_config(client, &config).await?;
            assert_eq!(state.admin, client.payer());
            assert_eq!(state.next_admin, None);
            assert_anchor_error(
                nominee
                    .accept_proxy_admin()
                    .send()
                    .await
                    .expect_err("withdrawn nominee cannot accept"),
                evm_proxy::ProxyError::NotPendingAdmin.into(),
            );
            assert_anchor_error(
                client
                    .transfer_proxy_admin(client.payer())
                    .send()
                    .await
                    .expect_err("withdrawal cannot be repeated"),
                evm_proxy::ProxyError::NoConfigChange.into(),
            );
            assert_eq!(read_config(client, &config).await?.0, before);
            Ok(())
        })
        .await
}
