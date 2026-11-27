use super::setup::{current_deployment, Deployment, EVM_PROXY_CONFIG_LOCK};
use futures_util::FutureExt;
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::AccountDeserialize;
use gmsol_sdk::{
    client::ops::{EvmProxyOps, SystemProgramOps},
    evm_proxy::ProxyCPIEvent,
    Client,
};
use gmsol_solana_utils::signer::{shared_signer, SignerRef};
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
};
use solana_transaction_status::UiTransactionEncoding;
use std::panic::{resume_unwind, AssertUnwindSafe};

type PolicyClient = Client<SignerRef>;

pub(super) async fn read_config(
    client: &PolicyClient,
    address: &Pubkey,
) -> eyre::Result<(Account, evm_proxy::Config)> {
    let account = client.rpc().get_account(address).await?;
    let state = evm_proxy::Config::try_deserialize(&mut account.data.as_slice())?;
    Ok((account, state))
}

pub(super) async fn event(
    client: &PolicyClient,
    signature: &Signature,
) -> eyre::Result<ProxyCPIEvent> {
    let transaction = client
        .rpc()
        .get_transaction_with_config(
            signature,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        )
        .await?;
    let mut events: Vec<_> = gmsol_sdk::evm_proxy::decode_committed_events(
        &evm_proxy::ID,
        transaction.slot,
        *signature,
        &transaction.transaction,
    )?
    .into_iter()
    .map(|event| event.into_data())
    .collect();
    eyre::ensure!(
        events.len() == 1,
        "expected one proxy CPI event, got {}",
        events.len()
    );
    Ok(events.remove(0))
}

pub(super) fn assert_anchor_error(error: gmsol_solana_utils::Error, expected: u32) {
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

const NOMINEE_FUNDING_LAMPORTS: u64 = 10_000_000;

pub(super) async fn assert_baseline(deployment: &Deployment) -> eyre::Result<evm_proxy::Config> {
    let (config, bump) = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID);
    let (account, state) = read_config(&deployment.client, &config).await?;
    assert_eq!(account.owner, evm_proxy::ID);
    assert_eq!(state.bump, bump);
    assert_eq!(state.store, deployment.store);
    assert_eq!(
        state.genesis_hash,
        deployment.client.rpc().get_genesis_hash().await?.to_bytes()
    );
    assert_eq!(state.admin, deployment.client.payer());
    assert_eq!(state.next_admin, None);
    assert!(state.paused);
    assert!(state.allowed_chain_ids.is_empty());
    Ok(state)
}

pub(super) async fn restore_baseline(
    deployment: &Deployment,
    nominees: &[&PolicyClient],
) -> eyre::Result<()> {
    let client = &deployment.client;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let mut state = read_config(client, &config).await?.1;
    if state.admin == client.payer() {
        if let Some(next) = state.next_admin {
            let nominee = nominees
                .iter()
                .find(|c| c.payer() == next)
                .ok_or_else(|| eyre::eyre!("cleanup lacks nominated admin signer"))?;
            nominee.accept_proxy_admin(evm_proxy::ID).send().await?;
            state = read_config(client, &config).await?.1;
        }
    }
    if state.admin != client.payer() {
        let admin = nominees
            .iter()
            .find(|c| c.payer() == state.admin)
            .ok_or_else(|| eyre::eyre!("cleanup lacks current admin signer"))?;
        if state.next_admin != Some(client.payer()) {
            admin
                .transfer_proxy_admin(evm_proxy::ID, client.payer())
                .send()
                .await?;
        }
        client.accept_proxy_admin(evm_proxy::ID).send().await?;
    }
    let state = read_config(client, &config).await?.1;
    if !state.paused {
        client.set_proxy_paused(evm_proxy::ID, true).send().await?;
    }
    for chain in state.allowed_chain_ids {
        client
            .set_proxy_chain_allowed(evm_proxy::ID, chain, false)
            .send()
            .await?;
    }
    assert_baseline(deployment).await?;
    Ok(())
}

pub(super) fn finish_policy_test(
    result: std::thread::Result<eyre::Result<()>>,
    restored: eyre::Result<()>,
) -> eyre::Result<()> {
    match result {
        Ok(result) => {
            restored?;
            result
        }
        Err(panic) => {
            if let Err(error) = restored {
                tracing::error!(%error, "Config cleanup failed after test panic");
            }
            resume_unwind(panic)
        }
    }
}

#[tokio::test]
async fn allowlist_enforces_authority_capacity_and_noops() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = EVM_PROXY_CONFIG_LOCK.lock().await;
    let _baseline = assert_baseline(&deployment).await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let result = AssertUnwindSafe(async {
        let first = chain_id(0x80, 1);
        let (before, _) = read_config(client, &config).await?;
        assert_anchor_error(
            other_client
                .set_proxy_chain_allowed(evm_proxy::ID, first, true)
                .send()
                .await
                .expect_err("non-admin chain change"),
            evm_proxy::ProxyError::NotConfigAdmin.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);

        let signature = client
            .set_proxy_chain_allowed(evm_proxy::ID, first, true)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigChainAllowedChanged(changed) = event(client, &signature).await?
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
                .set_proxy_chain_allowed(evm_proxy::ID, first, true)
                .send()
                .await
                .expect_err("duplicate chain add"),
            evm_proxy::ProxyError::NoConfigChange.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);

        let capacity = u8::try_from(evm_proxy::MAX_ALLOWED_CHAINS)?;
        for low in 2..=capacity {
            client
                .set_proxy_chain_allowed(evm_proxy::ID, chain_id(0x80, low), true)
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
                .set_proxy_chain_allowed(evm_proxy::ID, chain_id(0x80, capacity + 1), true)
                .send()
                .await
                .expect_err("chain capacity"),
            evm_proxy::ProxyError::AllowedChainsFull.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);
        let signature = client
            .set_proxy_chain_allowed(evm_proxy::ID, first, false)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigChainAllowedChanged(removed) = event(client, &signature).await?
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
                .set_proxy_chain_allowed(evm_proxy::ID, first, false)
                .send()
                .await
                .expect_err("duplicate chain removal"),
            evm_proxy::ProxyError::NoConfigChange.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);

        Ok::<(), eyre::Report>(())
    })
    .catch_unwind()
    .await;
    let restored = restore_baseline(&deployment, &[]).await;
    finish_policy_test(result, restored)
}

#[tokio::test]
async fn pause_changes_require_admin_and_reject_noops() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = EVM_PROXY_CONFIG_LOCK.lock().await;
    let _baseline = assert_baseline(&deployment).await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let result = AssertUnwindSafe(async {
        let (before, _) = read_config(client, &config).await?;
        assert_anchor_error(
            other_client
                .set_proxy_paused(evm_proxy::ID, false)
                .send()
                .await
                .expect_err("non-admin pause change"),
            evm_proxy::ProxyError::NotConfigAdmin.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);
        for (previous, paused) in [(true, false), (false, true)] {
            let signature = client
                .set_proxy_paused(evm_proxy::ID, paused)
                .send()
                .await?;
            let ProxyCPIEvent::ConfigPauseChanged(changed) = event(client, &signature).await?
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
                .set_proxy_paused(evm_proxy::ID, true)
                .send()
                .await
                .expect_err("pause no-op"),
            evm_proxy::ProxyError::NoConfigChange.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);

        Ok::<(), eyre::Report>(())
    })
    .catch_unwind()
    .await;
    let restored = restore_baseline(&deployment, &[]).await;
    finish_policy_test(result, restored)
}

#[tokio::test]
async fn handover_requires_latest_nominee_and_revokes_previous_admin() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = EVM_PROXY_CONFIG_LOCK.lock().await;
    let baseline = assert_baseline(&deployment).await?;
    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
    let genesis_hash = baseline.genesis_hash;
    let first_nominee = Keypair::new();
    let first_nominee_address = first_nominee.pubkey();
    let second_nominee = Keypair::new();
    let second_nominee_address = second_nominee.pubkey();
    for address in [first_nominee_address, second_nominee_address] {
        client
            .transfer(&address, NOMINEE_FUNDING_LAMPORTS)?
            .send()
            .await?;
    }
    let first_client = client.try_clone_with_payer(shared_signer(first_nominee))?;
    let second_client = client.try_clone_with_payer(shared_signer(second_nominee))?;
    let result = AssertUnwindSafe(async {
        let signature = client
            .transfer_proxy_admin(evm_proxy::ID, first_nominee_address)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigAdminProposed(proposal) = event(client, &signature).await? else {
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
                .transfer_proxy_admin(evm_proxy::ID, first_nominee_address)
                .send()
                .await
                .expect_err("same nomination"),
            evm_proxy::ProxyError::NoConfigChange.into(),
        );
        assert_anchor_error(
            client
                .transfer_proxy_admin(evm_proxy::ID, Pubkey::default())
                .send()
                .await
                .expect_err("zero nominee"),
            evm_proxy::ProxyError::InvalidAdmin.into(),
        );
        assert_anchor_error(
            first_client
                .set_proxy_paused(evm_proxy::ID, false)
                .send()
                .await
                .expect_err("pending admin has no authority"),
            evm_proxy::ProxyError::NotConfigAdmin.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);
        let signature = client.set_proxy_paused(evm_proxy::ID, false).send().await?;
        let ProxyCPIEvent::ConfigPauseChanged(old_admin_change) = event(client, &signature).await?
        else {
            eyre::bail!("expected ConfigPauseChanged CPI event");
        };
        assert_eq!(
            (old_admin_change.previous_paused, old_admin_change.paused),
            (true, false)
        );

        let signature = client
            .transfer_proxy_admin(evm_proxy::ID, second_nominee_address)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigAdminProposed(replacement) = event(client, &signature).await?
        else {
            eyre::bail!("expected ConfigAdminProposed CPI event");
        };
        assert_eq!(replacement.previous_next_admin, Some(first_nominee_address));
        assert_eq!(replacement.next_admin, second_nominee_address);
        let (before, _) = read_config(client, &config).await?;
        assert_anchor_error(
            first_client
                .accept_proxy_admin(evm_proxy::ID)
                .send()
                .await
                .expect_err("superseded nominee cannot accept"),
            evm_proxy::ProxyError::NotPendingAdmin.into(),
        );
        assert_anchor_error(
            other_client
                .accept_proxy_admin(evm_proxy::ID)
                .send()
                .await
                .expect_err("unrelated signer cannot accept"),
            evm_proxy::ProxyError::NotPendingAdmin.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);

        let signature = second_client
            .accept_proxy_admin(evm_proxy::ID)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigAdminAccepted(accepted) = event(client, &signature).await? else {
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
                .set_proxy_paused(evm_proxy::ID, true)
                .send()
                .await
                .expect_err("former admin lost authority"),
            evm_proxy::ProxyError::NotConfigAdmin.into(),
        );
        assert_anchor_error(
            second_client
                .accept_proxy_admin(evm_proxy::ID)
                .send()
                .await
                .expect_err("handover cannot be repeated"),
            evm_proxy::ProxyError::NotPendingAdmin.into(),
        );
        assert_eq!(read_config(client, &config).await?.0, before);
        let signature = second_client
            .set_proxy_paused(evm_proxy::ID, true)
            .send()
            .await?;
        let ProxyCPIEvent::ConfigPauseChanged(changed) = event(client, &signature).await? else {
            eyre::bail!("expected ConfigPauseChanged CPI event");
        };
        assert_eq!((changed.previous_paused, changed.paused), (false, true));
        let (_, state) = read_config(client, &config).await?;
        assert_eq!(state.store, deployment.store);
        assert_eq!(state.genesis_hash, genesis_hash);
        assert_eq!(state.admin, second_nominee_address);
        assert!(state.paused);
        Ok::<(), eyre::Report>(())
    })
    .catch_unwind()
    .await;
    let restored = restore_baseline(&deployment, &[&first_client, &second_client]).await;
    finish_policy_test(result, restored)
}
