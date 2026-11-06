use super::setup::Deployment;
use base64::Engine;
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{AccountDeserialize, AnchorDeserialize, Discriminator};
use gmsol_sdk::{client::ops::SystemProgramOps, Client};
use gmsol_solana_utils::{
    signer::{shared_signer, SignerRef},
    Result as TransactionResult,
};
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
};
use solana_transaction_status::{option_serializer::OptionSerializer, UiTransactionEncoding};

type PolicyClient = Client<SignerRef>;

async fn read_config(
    client: &PolicyClient,
    address: &Pubkey,
) -> eyre::Result<(Account, evm_proxy::Config)> {
    let account = client.rpc().get_account(address).await?;
    let state = evm_proxy::Config::try_deserialize(&mut account.data.as_slice())?;
    Ok((account, state))
}

async fn event<E: AnchorDeserialize + Discriminator>(
    client: &PolicyClient,
    signature: &Signature,
) -> eyre::Result<E> {
    let transaction = client
        .rpc()
        .get_transaction_with_config(
            signature,
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
        .ok_or_else(|| eyre::eyre!("transaction metadata absent"))?
        .log_messages
    {
        OptionSerializer::Some(logs) => logs,
        other => eyre::bail!("transaction logs unavailable: {other:?}"),
    };
    let mut events: Vec<E> = logs
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .filter_map(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .filter(|data| data.starts_with(E::DISCRIMINATOR))
        .map(|data| E::deserialize(&mut &data[E::DISCRIMINATOR.len()..]))
        .collect::<Result<_, _>>()?;
    eyre::ensure!(
        events.len() == 1,
        "expected one matching Config event, got {}",
        events.len()
    );
    Ok(events.remove(0))
}

fn set_chain(
    client: &PolicyClient,
    config: Pubkey,
    chain_id: [u8; 32],
    allowed: bool,
) -> impl std::future::Future<Output = TransactionResult<Signature>> + '_ {
    async move {
        client
            .store_transaction()
            .program(evm_proxy::ID)
            .anchor_args(evm_proxy::instruction::SetChainAllowed { chain_id, allowed })
            .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
                admin: client.payer(),
                config,
            })
            .send()
            .await
    }
}

fn set_paused(
    client: &PolicyClient,
    config: Pubkey,
    paused: bool,
) -> impl std::future::Future<Output = TransactionResult<Signature>> + '_ {
    async move {
        client
            .store_transaction()
            .program(evm_proxy::ID)
            .anchor_args(evm_proxy::instruction::SetPaused { paused })
            .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
                admin: client.payer(),
                config,
            })
            .send()
            .await
    }
}

fn propose(
    client: &PolicyClient,
    config: Pubkey,
    next_admin: Pubkey,
) -> impl std::future::Future<Output = TransactionResult<Signature>> + '_ {
    async move {
        client
            .store_transaction()
            .program(evm_proxy::ID)
            .anchor_args(evm_proxy::instruction::ProposeAdmin { next_admin })
            .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
                admin: client.payer(),
                config,
            })
            .send()
            .await
    }
}

fn accept(
    client: &PolicyClient,
    config: Pubkey,
) -> impl std::future::Future<Output = TransactionResult<Signature>> + '_ {
    async move {
        client
            .store_transaction()
            .program(evm_proxy::ID)
            .anchor_args(evm_proxy::instruction::AcceptAdmin {})
            .anchor_accounts(evm_proxy::accounts::AcceptConfigAdmin {
                next_admin: client.payer(),
                config,
            })
            .send()
            .await
    }
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
    id[31] = low;
    id
}

pub(super) async fn verify_policy_and_handover(
    deployment: &Deployment,
    config: Pubkey,
    genesis_hash: [u8; 32],
) -> eyre::Result<()> {
    use gmsol_programs::anchor_lang::error::ErrorCode;

    let client = &deployment.client;
    let other_client = deployment.user_client(Deployment::DEFAULT_KEEPER)?;
    let first = chain_id(0x80, 1);
    let (before, _) = read_config(client, &config).await?;
    assert_anchor_error(
        set_chain(&other_client, config, first, true)
            .await
            .expect_err("non-admin chain change"),
        ErrorCode::ConstraintHasOne.into(),
    );
    assert_eq!(read_config(client, &config).await?.0, before);

    let signature = set_chain(client, config, first, true).await?;
    let changed: evm_proxy::ConfigChainAllowedChanged = event(client, &signature).await?;
    assert_eq!(
        (changed.config, changed.chain_id, changed.allowed),
        (config, first, true)
    );
    let (before, state) = read_config(client, &config).await?;
    assert_eq!(state.allowed_chain_ids, vec![first]);
    assert_anchor_error(
        set_chain(client, config, first, true)
            .await
            .expect_err("duplicate chain add"),
        6003,
    );
    assert_eq!(read_config(client, &config).await?.0, before);

    for low in 2..=32 {
        set_chain(client, config, chain_id(0x80, low), true).await?;
    }
    let (before, state) = read_config(client, &config).await?;
    assert_eq!(state.allowed_chain_ids.len(), evm_proxy::MAX_ALLOWED_CHAINS);
    assert_eq!(state.allowed_chain_ids[0], first);
    assert_eq!(state.allowed_chain_ids[31], chain_id(0x80, 32));
    assert_anchor_error(
        set_chain(client, config, chain_id(0x80, 33), true)
            .await
            .expect_err("chain capacity"),
        6004,
    );
    assert_eq!(read_config(client, &config).await?.0, before);
    let signature = set_chain(client, config, first, false).await?;
    let removed: evm_proxy::ConfigChainAllowedChanged = event(client, &signature).await?;
    assert_eq!(
        (removed.config, removed.chain_id, removed.allowed),
        (config, first, false)
    );
    let (before, state) = read_config(client, &config).await?;
    assert_eq!(state.allowed_chain_ids.len(), 31);
    assert_anchor_error(
        set_chain(client, config, first, false)
            .await
            .expect_err("duplicate chain removal"),
        6003,
    );
    assert_eq!(read_config(client, &config).await?.0, before);

    for (previous, paused) in [(true, false), (false, true)] {
        let signature = set_paused(client, config, paused).await?;
        let changed: evm_proxy::ConfigPauseChanged = event(client, &signature).await?;
        assert_eq!(
            (changed.config, changed.previous_paused, changed.paused),
            (config, previous, paused)
        );
        assert_eq!(read_config(client, &config).await?.1.paused, paused);
    }
    let (before, _) = read_config(client, &config).await?;
    assert_anchor_error(
        set_paused(client, config, true)
            .await
            .expect_err("pause no-op"),
        6003,
    );
    assert_eq!(read_config(client, &config).await?.0, before);

    let first_nominee = Keypair::new();
    let first_nominee_address = first_nominee.pubkey();
    let second_nominee = Keypair::new();
    let second_nominee_address = second_nominee.pubkey();
    for address in [first_nominee_address, second_nominee_address] {
        client.transfer(&address, 10_000_000)?.send().await?;
    }
    let first_client = client.try_clone_with_payer(shared_signer(first_nominee))?;
    let second_client = client.try_clone_with_payer(shared_signer(second_nominee))?;
    let signature = propose(client, config, first_nominee_address).await?;
    let proposal: evm_proxy::ConfigAdminProposed = event(client, &signature).await?;
    assert_eq!(proposal.config, config);
    assert_eq!(proposal.admin, client.payer());
    assert_eq!(proposal.previous_pending_admin, None);
    assert_eq!(proposal.next_admin, first_nominee_address);
    let (before, state) = read_config(client, &config).await?;
    assert_eq!(state.admin, client.payer());
    assert_eq!(state.pending_admin, Some(first_nominee_address));
    assert_anchor_error(
        propose(client, config, first_nominee_address)
            .await
            .expect_err("same nomination"),
        6003,
    );
    assert_anchor_error(
        propose(client, config, Pubkey::default())
            .await
            .expect_err("zero nominee"),
        6002,
    );
    assert_anchor_error(
        set_paused(&first_client, config, false)
            .await
            .expect_err("pending admin has no authority"),
        ErrorCode::ConstraintHasOne.into(),
    );
    assert_eq!(read_config(client, &config).await?.0, before);
    let signature = set_paused(client, config, false).await?;
    let old_admin_change: evm_proxy::ConfigPauseChanged = event(client, &signature).await?;
    assert_eq!(
        (old_admin_change.previous_paused, old_admin_change.paused),
        (true, false)
    );

    let signature = propose(client, config, second_nominee_address).await?;
    let replacement: evm_proxy::ConfigAdminProposed = event(client, &signature).await?;
    assert_eq!(
        replacement.previous_pending_admin,
        Some(first_nominee_address)
    );
    assert_eq!(replacement.next_admin, second_nominee_address);
    let (before, _) = read_config(client, &config).await?;
    assert_anchor_error(
        accept(&first_client, config)
            .await
            .expect_err("superseded nominee cannot accept"),
        6005,
    );
    assert_anchor_error(
        accept(&other_client, config)
            .await
            .expect_err("unrelated signer cannot accept"),
        6005,
    );
    assert_eq!(read_config(client, &config).await?.0, before);

    let signature = accept(&second_client, config).await?;
    let accepted: evm_proxy::ConfigAdminAccepted = event(client, &signature).await?;
    assert_eq!(accepted.config, config);
    assert_eq!(accepted.previous_admin, client.payer());
    assert_eq!(accepted.admin, second_nominee_address);
    assert_eq!(accepted.pending_admin, None);
    let (before, state) = read_config(client, &config).await?;
    assert_eq!(state.admin, second_nominee_address);
    assert_eq!(state.pending_admin, None);
    assert_eq!(state.store, deployment.store);
    assert_eq!(state.genesis_hash, genesis_hash);
    assert_anchor_error(
        set_paused(client, config, true)
            .await
            .expect_err("former admin lost authority"),
        ErrorCode::ConstraintHasOne.into(),
    );
    assert_anchor_error(
        accept(&second_client, config)
            .await
            .expect_err("handover cannot be repeated"),
        6005,
    );
    assert_eq!(read_config(client, &config).await?.0, before);
    let signature = set_paused(&second_client, config, true).await?;
    let changed: evm_proxy::ConfigPauseChanged = event(client, &signature).await?;
    assert_eq!((changed.previous_paused, changed.paused), (false, true));
    let (_, state) = read_config(client, &config).await?;
    assert_eq!(state.store, deployment.store);
    assert_eq!(state.genesis_hash, genesis_hash);
    assert_eq!(state.admin, second_nominee_address);
    assert!(state.paused);
    Ok(())
}
