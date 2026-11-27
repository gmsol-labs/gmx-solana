use super::setup::current_deployment;
use eyre::OptionExt;
use futures_util::{stream, StreamExt};
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::AccountDeserialize;
use gmsol_sdk::decode::{gmsol::programs::evm_proxy::EvmProxyCPIEvent, Decode};
use gmsol_sdk::{
    client::{
        ops::{EvmProxyOps, SystemProgramOps},
        transaction_history::extract_cpi_events,
    },
    evm_proxy::CancelIntent,
    Client,
};
use gmsol_solana_utils::{
    client::SendAndConfirm,
    signer::{shared_signer, SignerRef},
    transaction_builder::default_before_sign,
    utils::WithSlot,
};
use libsecp256k1::{Message, PublicKey, SecretKey};
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    clock::Clock,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_program,
};
use solana_transaction_status::UiTransactionEncoding;

fn random_signing_key() -> SecretKey {
    loop {
        if let Ok(key) = SecretKey::parse(&rand::random()) {
            return key;
        }
    }
}

fn signed(
    intent: &CancelIntent,
    store: Pubkey,
    genesis_hash: [u8; 32],
    key: &SecretKey,
) -> ([u8; 64], u8) {
    let digest = intent.digest(&evm_proxy::ID, &store, &genesis_hash);
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), key);
    (signature.serialize(), recovery.serialize())
}

fn high_s_counterpart(signature: [u8; 64], recovery_id: u8) -> ([u8; 64], u8) {
    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];
    const HALF_ORDER: [u8; 32] = [
        0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b,
        0x20, 0xa0,
    ];
    let low_s: [u8; 32] = signature[32..].try_into().expect("32-byte s scalar");
    assert!(low_s != [0; 32] && low_s <= HALF_ORDER);
    let mut counterpart = signature;
    let mut borrow = 0u16;
    for index in (0..32).rev() {
        let left = u16::from(ORDER[index]);
        let right = u16::from(low_s[index]) + borrow;
        counterpart[32 + index] = ((left + 256 - right) % 256) as u8;
        borrow = u16::from(left < right);
    }
    assert_eq!(borrow, 0);
    let high_s: [u8; 32] = counterpart[32..].try_into().expect("32-byte s scalar");
    assert!(high_s > HALF_ORDER && high_s < ORDER);
    (counterpart, recovery_id ^ 1)
}

fn owner_address(key: &SecretKey) -> [u8; 20] {
    let public = PublicKey::from_secret_key(key).serialize();
    let digest = solana_sdk::keccak::hash(&public[1..]).to_bytes();
    digest[12..].try_into().expect("20-byte EVM address")
}

fn assert_only_next_nonce_changed(
    before: &solana_sdk::account::Account,
    after: &solana_sdk::account::Account,
    expected_next_nonce: u64,
) -> eyre::Result<()> {
    let previous = evm_proxy::Proxy::try_deserialize(&mut before.data.as_slice())?;
    let current = evm_proxy::Proxy::try_deserialize(&mut after.data.as_slice())?;
    assert_eq!(current.next_nonce, expected_next_nonce);
    assert_eq!(current.version, previous.version);
    assert_eq!(current.bump, previous.bump);
    assert_eq!(current.padding_0, previous.padding_0);
    assert_eq!(current.evm_address, previous.evm_address);
    assert_eq!(current.padding_1, previous.padding_1);
    assert_eq!(current.reserved, previous.reserved);
    assert_eq!(after.owner, before.owner);
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(after.executable, before.executable);
    assert_eq!(after.rent_epoch, before.rent_epoch);
    assert_eq!(after.data.len(), before.data.len());
    Ok(())
}

async fn cpi_event(
    client: &Client<SignerRef>,
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

#[tokio::test]
async fn cancel_verifies_domain_signature_policy_and_preserves_consumed_nonce() -> eyre::Result<()>
{
    const SUBMITTER_FUNDING_LAMPORTS: u64 = 2_000_000_000;
    const INTENT_VALIDITY_SECONDS: u64 = 120;
    const OUTSIDE_EXECUTION_WINDOW_SECONDS: u64 = 3600 + 120;
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    deployment
        .with_evm_proxy_config(&[], async {
            let config = gmsol_sdk::pda::find_proxy_config_address(&evm_proxy::ID).0;
            let genesis_hash = deployment.client.proxy_config_account().await?.genesis_hash;
            let admin = &deployment.client;
            let payer = Keypair::new();
            admin
                .transfer(&payer.pubkey(), SUBMITTER_FUNDING_LAMPORTS)?
                .send()
                .await?;
            let submitter = admin.try_clone_with_payer(shared_signer(payer))?;
            let owner = random_signing_key();
            let evm_address = owner_address(&owner);
            let (proxy, _) = gmsol_sdk::pda::find_proxy_address(&evm_address, &evm_proxy::ID);
            submitter
                .store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
                .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
                    payer: submitter.payer(),
                    proxy,
                    system_program: system_program::ID,
                })
                .send()
                .await?;
            let before = admin.rpc().get_account(&proxy).await?;
            assert_eq!(before.owner, evm_proxy::ID);
            let initial = evm_proxy::Proxy::try_deserialize(&mut before.data.as_slice())?;
            assert_eq!(initial.next_nonce, 0);

            let mut chain_id = [0; 32];
            chain_id[chain_id.len() - 1] = 1;
            let clock_account = admin
                .rpc()
                .get_account_with_commitment(
                    &solana_sdk::sysvar::clock::ID,
                    CommitmentConfig::processed(),
                )
                .await?
                .value
                .ok_or_else(|| eyre::eyre!("processed Clock account unavailable"))?;
            let clock: Clock = bincode::deserialize(&clock_account.data)?;
            let deadline = u64::try_from(clock.unix_timestamp)?
                .checked_add(INTENT_VALIDITY_SECONDS)
                .expect("deadline overflow");
            let intent = CancelIntent {
                evm_address,
                chain_id,
                nonce: 0,
                deadline,
            };
            let (signature, recovery_id) = signed(&intent, deployment.store, genesis_hash, &owner);

            let paused = submitter
                .cancel_proxy_intent(&intent, signature, recovery_id)?
                .send()
                .await
                .expect_err("Config starts paused");
            assert_eq!(
                gmsol_sdk::Error::from(paused).anchor_error_code(),
                Some(evm_proxy::ProxyError::Paused.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            admin.set_proxy_paused(false).send().await?;
            let disallowed = submitter
                .cancel_proxy_intent(&intent, signature, recovery_id)?
                .send()
                .await
                .expect_err("chain is not allowed");
            assert_eq!(
                gmsol_sdk::Error::from(disallowed).anchor_error_code(),
                Some(evm_proxy::ProxyError::ChainNotAllowed.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            admin
                .set_proxy_chain_allowed(chain_id, true)
                .send()
                .await?;
            let wrong_owner = random_signing_key();
            let (wrong_signature, wrong_recovery) =
                signed(&intent, deployment.store, genesis_hash, &wrong_owner);
            let wrong = submitter
                .cancel_proxy_intent(&intent, wrong_signature, wrong_recovery)?
                .send()
                .await
                .expect_err("wrong EVM signer");
            assert_eq!(
                gmsol_sdk::Error::from(wrong).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidSignature.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            for (label, store, cluster_hash) in [
                ("different Store", Pubkey::new_unique(), genesis_hash),
                ("different cluster", deployment.store, [0x42; 32]),
            ] {
                let (foreign_signature, foreign_recovery) =
                    signed(&intent, store, cluster_hash, &owner);
                let error = submitter
                    .cancel_proxy_intent(
                        &intent,
                        foreign_signature,
                        foreign_recovery,
                    )?
                    .send()
                    .await
                    .expect_err(label);
                assert_eq!(
                    gmsol_sdk::Error::from(error).anchor_error_code(),
                    Some(evm_proxy::ProxyError::InvalidSignature.into())
                );
                assert_eq!(admin.rpc().get_account(&proxy).await?, before);
            }

            for (label, offset) in [
                ("expired deadline", None),
                ("deadline beyond execution window", Some(OUTSIDE_EXECUTION_WINDOW_SECONDS)),
            ] {
                let invalid_deadline = if let Some(offset) = offset {
                    let account = admin.rpc().get_account_with_commitment(
                        &solana_sdk::sysvar::clock::ID,
                        CommitmentConfig::processed(),
                    ).await?.value.ok_or_else(|| eyre::eyre!("processed Clock account unavailable"))?;
                    let clock: Clock = bincode::deserialize(&account.data)?;
                    u64::try_from(clock.unix_timestamp)?.checked_add(offset).expect("deadline overflow")
                } else {
                    0
                };
                let invalid = CancelIntent {
                    deadline: invalid_deadline,
                    ..intent
                };
                let (invalid_signature, invalid_recovery) =
                    signed(&invalid, deployment.store, genesis_hash, &owner);
                let error = submitter
                    .cancel_proxy_intent(
                        &invalid,
                        invalid_signature,
                        invalid_recovery,
                    )?
                    .send()
                    .await
                    .expect_err(label);
                assert_eq!(
                    gmsol_sdk::Error::from(error).anchor_error_code(),
                    Some(evm_proxy::ProxyError::InvalidDeadline.into())
                );
                assert_eq!(admin.rpc().get_account(&proxy).await?, before);
            }

            let (high_signature, high_recovery) = high_s_counterpart(signature, recovery_id);
            let digest = intent.digest(&evm_proxy::ID, &deployment.store, &genesis_hash);
            let recovered =
                gmsol_programs::anchor_lang::solana_program::secp256k1_recover::secp256k1_recover(
                    &digest,
                    high_recovery,
                    &high_signature,
                )
                .expect("high-s counterpart must recover without the program's low-s policy");
            let public = PublicKey::from_secret_key(&owner).serialize();
            assert_eq!(&recovered.to_bytes()[..], &public[1..]);
            let high_s_error = submitter
                .store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::Cancel {
                    chain_id,
                    nonce: 0,
                    deadline,
                    signature: high_signature,
                    recovery_id: high_recovery,
                })
                .anchor_accounts(evm_proxy::accounts::Cancel {
                    submitter: submitter.payer(),
                    config,
                    proxy,
                    event_authority: gmsol_sdk::builders::evm_proxy::EvmProxyProgram::from(evm_proxy::ID).find_event_authority_address(),
                    program: evm_proxy::ID,
                })
                .send()
                .await
                .expect_err("otherwise valid high-s signature");
            assert_eq!(
                gmsol_sdk::Error::from(high_s_error).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidSignature.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            let bad_recovery = submitter
                .store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::Cancel {
                    chain_id,
                    nonce: 0,
                    deadline,
                    signature,
                    recovery_id: 27,
                })
                .anchor_accounts(evm_proxy::accounts::Cancel {
                    submitter: submitter.payer(),
                    config,
                    proxy,
                    event_authority: gmsol_sdk::builders::evm_proxy::EvmProxyProgram::from(evm_proxy::ID).find_event_authority_address(),
                    program: evm_proxy::ID,
                })
                .send()
                .await
                .expect_err("noncanonical recovery ID");
            assert_eq!(
                gmsol_sdk::Error::from(bad_recovery).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidSignature.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            let forged = CancelIntent { nonce: 2, ..intent };
            let (forged_signature, forged_recovery) =
                signed(&forged, deployment.store, genesis_hash, &owner);
            let future = submitter
                .cancel_proxy_intent(&forged, forged_signature, forged_recovery)?
                .send()
                .await
                .expect_err("future nonce");
            assert_eq!(
                gmsol_sdk::Error::from(future).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidIntentNonce.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            let other_address = [0x69; 20];
            let (other_proxy, _) = gmsol_sdk::pda::find_proxy_address(&other_address, &evm_proxy::ID);
            submitter
                .store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::CreateProxyIdempotent {
                    evm_address: other_address,
                })
                .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
                    payer: submitter.payer(),
                    proxy: other_proxy,
                    system_program: system_program::ID,
                })
                .send()
                .await?;
            let other_before = admin.rpc().get_account(&other_proxy).await?;
            let mut substituted = gmsol_sdk::evm_proxy::cancel_instruction(
                submitter.evm_proxy_program_for_builders(),
                submitter.payer(),
                &intent,
                signature,
                recovery_id,
            )?;
            substituted.accounts[2].pubkey = other_proxy;
            let wrong_proxy = submitter
                .program(substituted.program_id)
                .transaction()
                .accounts(substituted.accounts)
                .args(substituted.data)
                .send()
                .await
                .expect_err("valid different proxy rejects the original owner signature");
            assert_eq!(
                gmsol_sdk::Error::from(wrong_proxy).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidSignature.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);
            assert_eq!(admin.rpc().get_account(&other_proxy).await?, other_before);

            // This failure must land on the validator: simulation alone cannot prove
            // that successful CPI traces from a failed transaction are not committed.
            let rollback_owner = random_signing_key();
            let rollback_address = owner_address(&rollback_owner);
            let rollback_proxy = gmsol_sdk::pda::find_proxy_address(&rollback_address, &evm_proxy::ID).0;
            assert!(admin.rpc().get_account_with_commitment(&rollback_proxy, CommitmentConfig::confirmed()).await?.value.is_none());
            let rollback_intent = CancelIntent { evm_address: rollback_address, ..intent };
            let (rollback_signature, rollback_recovery) = signed(
                &rollback_intent, deployment.store, genesis_hash, &rollback_owner,
            );
            let create = submitter.store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address: rollback_address })
                .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
                    payer: submitter.payer(), proxy: rollback_proxy, system_program: system_program::ID,
                }).take_instruction().expect("proxy creation instruction");
            let cancel = gmsol_sdk::evm_proxy::cancel_instruction(
                submitter.evm_proxy_program_for_builders(), submitter.payer(), &rollback_intent, rollback_signature, rollback_recovery,
            )?;
            let fail = solana_sdk::system_instruction::transfer(
                &submitter.payer(), &Pubkey::new_unique(), u64::MAX,
            );
            let rollback_builder = submitter.store_transaction()
                .pre_instruction(create, true)
                .pre_instruction(cancel, true)
                .pre_instruction(fail, true);
            let transaction = rollback_builder.signed_transaction_with_options(
                false, None, None, gmsol_solana_utils::transaction_builder::default_before_sign,
            ).await?;
            let failing_index = u8::try_from(transaction.message.instructions().len() - 1)?;
            let on_chain_signature = transaction.signatures[0];
            let payer_before = admin.rpc().get_balance(&submitter.payer()).await?;
            submitter.rpc().send_and_confirm_transaction_with_config(
                &transaction,
                RpcSendTransactionConfig { skip_preflight: true, ..Default::default() },
            ).await.expect_err("later instruction must fail after the valid Cancel");
            // Confirmation can precede RPC history indexing. Keep the wait bounded
            // and propagate its final error.
            const METADATA_RETRIES: usize = 19;
            const METADATA_RETRY_MILLIS: u64 = 250;
            let mut attempt = 0;
            let failed = loop {
                match admin.rpc().get_transaction_with_config(
                    &on_chain_signature,
                    RpcTransactionConfig {
                        encoding: Some(UiTransactionEncoding::Base64),
                        commitment: Some(CommitmentConfig::confirmed()),
                        max_supported_transaction_version: Some(0),
                    },
                ).await {
                    Ok(observed) => break observed,
                    Err(_) if attempt < METADATA_RETRIES => {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(METADATA_RETRY_MILLIS)).await;
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            let meta = failed.transaction.meta.as_ref().expect("landed transaction metadata");
            assert!(matches!(meta.err, Some(TransactionError::InstructionError(index, _)) if index == failing_index));
            let mut trace_decoder = gmsol_sdk::decode::decoder::TransactionDecoder::new(
                failed.slot, on_chain_signature, &failed.transaction,
            );
            trace_decoder.add_cpi_event_program_id(&evm_proxy::ID)?;
            let traces = gmsol_sdk::decode::value::AnchorCPIEvents::<EvmProxyCPIEvent>::decode(&trace_decoder)?;
            assert_eq!(traces.events().len(), 1, "valid intent emitted before the later failure");
            assert!(matches!(traces.events()[0].data(), EvmProxyCPIEvent::IntentConsumed(event)
                if event.proxy == rollback_proxy && event.nonce == 0 && event.kind == evm_proxy::INTENT_KIND_CANCEL));
            assert!(admin.rpc().get_account_with_commitment(&rollback_proxy, CommitmentConfig::confirmed()).await?.value.is_none(),
                "proxy creation rent and nonce write must both roll back");
            assert_eq!(admin.rpc().get_balance(&submitter.payer()).await?, payer_before - meta.fee,
                "rent rolls back while transaction fees remain charged");
            assert_eq!(admin.rpc().get_account(&proxy).await?, before);

            let clock_account = admin.rpc().get_account_with_commitment(
                &solana_sdk::sysvar::clock::ID, CommitmentConfig::processed(),
            ).await?.value.ok_or_else(|| eyre::eyre!("processed Clock account unavailable"))?;
            let clock: Clock = bincode::deserialize(&clock_account.data)?;
            let deadline = u64::try_from(clock.unix_timestamp)?.checked_add(INTENT_VALIDITY_SECONDS).expect("deadline overflow");
            let intent = CancelIntent { deadline, ..intent };
            let (signature, recovery_id) = signed(&intent, deployment.store, genesis_hash, &owner);
            let competing = CancelIntent {
                deadline: deadline + 1,
                ..intent
            };
            let (competing_signature, competing_recovery) =
                signed(&competing, deployment.store, genesis_hash, &owner);
            let signature_on_chain = submitter
                .cancel_proxy_intent(&intent, signature, recovery_id)?
                .send_with_options(false, None, None, Default::default(), default_before_sign)
                .await?;
            let after = admin.rpc().get_account(&proxy).await?;
            let consumed = evm_proxy::Proxy::try_deserialize(&mut after.data.as_slice())?;
            assert_eq!(consumed.next_nonce, 1);
            let sdk_proxy = gmsol_programs::gmsol_evm_proxy::accounts::Proxy::try_deserialize(
                &mut after.data.as_slice(),
            )?;
            assert_eq!(sdk_proxy.next_nonce, 1);
            assert_only_next_nonce_changed(&before, &after, 1)?;
            let EvmProxyCPIEvent::IntentConsumed(event) =
                cpi_event(admin, signature_on_chain).await? else { eyre::bail!("expected consumed Cancel CPI event"); };
            assert_eq!(
                (event.proxy, event.nonce, event.kind),
                (proxy, intent.nonce, evm_proxy::INTENT_KIND_CANCEL)
            );

            // A different funded payer must preserve already consumed state on recreation.
            admin
                .store_transaction()
                .program(evm_proxy::ID)
                .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
                .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
                    payer: admin.payer(),
                    proxy,
                    system_program: system_program::ID,
                })
                .send()
                .await?;
            assert_ne!(admin.payer(), submitter.payer());
            assert_eq!(
                admin.rpc().get_account(&proxy).await?,
                after,
                "recreation must preserve consumed nonce and full proxy account"
            );

            let replay = admin
                .cancel_proxy_intent(&intent, signature, recovery_id)?
                .send()
                .await
                .expect_err("consumed nonce replay");
            assert_eq!(
                gmsol_sdk::Error::from(replay).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidIntentNonce.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, after);

            let canceled = submitter
                .cancel_proxy_intent(
                    &competing,
                    competing_signature,
                    competing_recovery,
                )?
                .send()
                .await
                .expect_err("another signed message at the canceled nonce");
            assert_eq!(
                gmsol_sdk::Error::from(canceled).anchor_error_code(),
                Some(evm_proxy::ProxyError::InvalidIntentNonce.into())
            );
            assert_eq!(admin.rpc().get_account(&proxy).await?, after);

            let clock_account = admin.rpc().get_account_with_commitment(
                &solana_sdk::sysvar::clock::ID, CommitmentConfig::processed(),
            ).await?.value.ok_or_else(|| eyre::eyre!("processed Clock account unavailable"))?;
            let clock: Clock = bincode::deserialize(&clock_account.data)?;
            let deadline = u64::try_from(clock.unix_timestamp)?.checked_add(INTENT_VALIDITY_SECONDS).expect("deadline overflow");
            let following = CancelIntent {
                deadline,
                nonce: intent.nonce + 1,
                ..intent
            };
            let (following_signature, following_recovery) =
                signed(&following, deployment.store, genesis_hash, &owner);
            let second_signature = submitter
                .cancel_proxy_intent(
                    &following,
                    following_signature,
                    following_recovery,
                )?
                .send_with_options(false, None, None, Default::default(), default_before_sign)
                .await?;
            let after_second = admin.rpc().get_account(&proxy).await?;
            assert_only_next_nonce_changed(&after, &after_second, 2)?;
            let EvmProxyCPIEvent::IntentConsumed(second_event) =
                cpi_event(admin, second_signature).await? else { eyre::bail!("expected consumed Cancel CPI event"); };
            assert_eq!(
                (second_event.proxy, second_event.nonce, second_event.kind),
                (proxy, following.nonce, evm_proxy::INTENT_KIND_CANCEL)
            );

            Ok(())
        })
        .await
}
