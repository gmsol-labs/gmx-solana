use super::setup::Deployment;
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::AccountDeserialize;
use gmsol_sdk::{client::ops::SystemProgramOps, evm_proxy::CancelIntent};
use gmsol_solana_utils::signer::shared_signer;
use libsecp256k1::{Message, PublicKey, SecretKey};
use solana_sdk::{
    clock::Clock,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
};

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

fn old_pair_signed(
    intent: &CancelIntent,
    store: Pubkey,
    genesis_hash: [u8; 32],
    key: &SecretKey,
) -> ([u8; 64], u8) {
    use solana_sdk::keccak;

    let typed = intent.typed_data(&evm_proxy::ID, &store, &genesis_hash);
    let domain = keccak::hashv(&[
        keccak::hash(b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)")
            .as_ref(),
        keccak::hash(typed.name.as_bytes()).as_ref(),
        keccak::hash(typed.version.as_bytes()).as_ref(),
        &typed.chain_id,
        &typed.salt,
    ]);
    let mut nonce = [0; 32];
    nonce[24..].copy_from_slice(&intent.nonce.to_be_bytes());
    let mut deadline = [0; 32];
    deadline[24..].copy_from_slice(&intent.deadline.to_be_bytes());
    let structure = keccak::hashv(&[
        keccak::hash(
            b"Cancel(bytes32 proxyAccount,uint64 generation,uint64 nonce,uint64 deadline)",
        )
        .as_ref(),
        typed.proxy_account.as_ref(),
        &[0; 32],
        &nonce,
        &deadline,
    ]);
    let digest = keccak::hashv(&[b"\x19\x01", domain.as_ref(), structure.as_ref()]);
    assert_ne!(
        digest.to_bytes(),
        intent.digest(&evm_proxy::ID, &store, &genesis_hash)
    );
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest.to_bytes()), key);
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

/// Exercise the production Cancel path while the Config lifecycle owns its singleton.
pub(super) async fn verify_signed_cancel(
    deployment: &Deployment,
    config: Pubkey,
    genesis_hash: [u8; 32],
) -> eyre::Result<()> {
    let admin = &deployment.client;
    let payer = Keypair::new();
    admin
        .transfer(&payer.pubkey(), 2_000_000_000)?
        .send()
        .await?;
    let submitter = admin.try_clone_with_payer(shared_signer(payer))?;
    let owner = SecretKey::random(&mut rand_08::rngs::OsRng);
    let evm_address = owner_address(&owner);
    let (proxy, _) = gmsol_sdk::pda::find_proxy_address(&evm_address, &evm_proxy::ID);
    submitter
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy { evm_address })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: submitter.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let before = admin.rpc().get_account(&proxy).await?;
    assert_eq!(before.owner, evm_proxy::ID);
    let initial = evm_proxy::Proxy::try_deserialize(&mut before.data.as_slice())?;
    assert_eq!(initial.consumed_nonce(), 0);

    let mut chain_id = [0; 32];
    chain_id[31] = 1;
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
        .checked_add(120)
        .expect("deadline overflow");
    let intent = CancelIntent {
        evm_address,
        chain_id,
        nonce: 1,
        deadline,
    };
    let (signature, recovery_id) = signed(&intent, deployment.store, genesis_hash, &owner);

    let paused = submitter
        .cancel_proxy_transaction(&intent, signature, recovery_id)?
        .send()
        .await
        .expect_err("Config starts paused");
    assert_eq!(
        gmsol_sdk::Error::from(paused).anchor_error_code(),
        Some(6008)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    admin
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::SetPaused { paused: false })
        .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
            admin: admin.payer(),
            config,
        })
        .send()
        .await?;
    let disallowed = submitter
        .cancel_proxy_transaction(&intent, signature, recovery_id)?
        .send()
        .await
        .expect_err("chain is not allowed");
    assert_eq!(
        gmsol_sdk::Error::from(disallowed).anchor_error_code(),
        Some(6009)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    admin
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::SetChainAllowed {
            chain_id,
            allowed: true,
        })
        .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
            admin: admin.payer(),
            config,
        })
        .send()
        .await?;
    let wrong_owner = SecretKey::random(&mut rand_08::rngs::OsRng);
    let (old_signature, old_recovery) =
        old_pair_signed(&intent, deployment.store, genesis_hash, &owner);
    let old_schema = submitter
        .cancel_proxy_transaction(&intent, old_signature, old_recovery)?
        .send()
        .await
        .expect_err("old pair-schema signature");
    assert_eq!(
        gmsol_sdk::Error::from(old_schema).anchor_error_code(),
        Some(6011)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    let (wrong_signature, wrong_recovery) =
        signed(&intent, deployment.store, genesis_hash, &wrong_owner);
    let wrong = submitter
        .cancel_proxy_transaction(&intent, wrong_signature, wrong_recovery)?
        .send()
        .await
        .expect_err("wrong EVM signer");
    assert_eq!(
        gmsol_sdk::Error::from(wrong).anchor_error_code(),
        Some(6011)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    for (label, store, cluster_hash) in [
        ("different Store", Pubkey::new_unique(), genesis_hash),
        ("different cluster", deployment.store, [0x42; 32]),
    ] {
        let (foreign_signature, foreign_recovery) = signed(&intent, store, cluster_hash, &owner);
        let error = submitter
            .cancel_proxy_transaction(&intent, foreign_signature, foreign_recovery)?
            .send()
            .await
            .expect_err(label);
        assert_eq!(
            gmsol_sdk::Error::from(error).anchor_error_code(),
            Some(6011)
        );
        assert_eq!(admin.rpc().get_account(&proxy).await?, before);
    }

    for (label, invalid_deadline) in [
        ("expired deadline", 0),
        ("deadline beyond execution window", deadline + 3601),
    ] {
        let invalid = CancelIntent {
            deadline: invalid_deadline,
            ..intent
        };
        let (invalid_signature, invalid_recovery) =
            signed(&invalid, deployment.store, genesis_hash, &owner);
        let error = submitter
            .cancel_proxy_transaction(&invalid, invalid_signature, invalid_recovery)?
            .send()
            .await
            .expect_err(label);
        assert_eq!(
            gmsol_sdk::Error::from(error).anchor_error_code(),
            Some(6010)
        );
        assert_eq!(admin.rpc().get_account(&proxy).await?, before);
    }

    let malformed = submitter
        .cancel_proxy_transaction(&intent, [0xff; 64], recovery_id)?
        .send()
        .await
        .expect_err("noncanonical signature scalar");
    assert_eq!(
        gmsol_sdk::Error::from(malformed).anchor_error_code(),
        Some(6011)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

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
            evm_address,
            chain_id,
            nonce: 1,
            deadline,
            signature: high_signature,
            recovery_id: high_recovery,
        })
        .anchor_accounts(evm_proxy::accounts::Cancel {
            submitter: submitter.payer(),
            config,
            proxy,
        })
        .send()
        .await
        .expect_err("otherwise valid high-s signature");
    assert_eq!(
        gmsol_sdk::Error::from(high_s_error).anchor_error_code(),
        Some(6011)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    let bad_recovery = submitter
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::Cancel {
            evm_address,
            chain_id,
            nonce: 1,
            deadline,
            signature,
            recovery_id: 27,
        })
        .anchor_accounts(evm_proxy::accounts::Cancel {
            submitter: submitter.payer(),
            config,
            proxy,
        })
        .send()
        .await
        .expect_err("noncanonical recovery ID");
    assert_eq!(
        gmsol_sdk::Error::from(bad_recovery).anchor_error_code(),
        Some(6011)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    let forged = CancelIntent { nonce: 2, ..intent };
    let (forged_signature, forged_recovery) =
        signed(&forged, deployment.store, genesis_hash, &owner);
    let future = submitter
        .cancel_proxy_transaction(&forged, forged_signature, forged_recovery)?
        .send()
        .await
        .expect_err("future nonce");
    assert_eq!(
        gmsol_sdk::Error::from(future).anchor_error_code(),
        Some(6006)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    let mut wrong_config = gmsol_sdk::evm_proxy::cancel_instruction(
        evm_proxy::ID,
        submitter.payer(),
        &intent,
        signature,
        recovery_id,
    )?;
    wrong_config.accounts[1].pubkey = submitter.payer();
    let wrong_pda = submitter
        .evm_proxy_instruction(wrong_config)
        .send()
        .await
        .expect_err("system-owned account cannot substitute for Config");
    assert_eq!(
        gmsol_sdk::Error::from(wrong_pda).anchor_error_code(),
        Some(gmsol_programs::anchor_lang::error::ErrorCode::AccountOwnedByWrongProgram.into())
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);

    let other_address = [0x69; 20];
    let (other_proxy, _) = gmsol_sdk::pda::find_proxy_address(&other_address, &evm_proxy::ID);
    submitter
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::CreateProxy {
            evm_address: other_address,
        })
        .anchor_accounts(evm_proxy::accounts::CreateProxy {
            payer: submitter.payer(),
            proxy: other_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let other_before = admin.rpc().get_account(&other_proxy).await?;
    let mut substituted = gmsol_sdk::evm_proxy::cancel_instruction(
        evm_proxy::ID,
        submitter.payer(),
        &intent,
        signature,
        recovery_id,
    )?;
    substituted.accounts[2].pubkey = other_proxy;
    let wrong_proxy = submitter
        .evm_proxy_instruction(substituted)
        .send()
        .await
        .expect_err("different proxy PDA");
    assert_eq!(
        gmsol_sdk::Error::from(wrong_proxy).anchor_error_code(),
        Some(2006)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, before);
    assert_eq!(admin.rpc().get_account(&other_proxy).await?, other_before);

    let competing = CancelIntent {
        deadline: deadline + 1,
        ..intent
    };
    let (competing_signature, competing_recovery) =
        signed(&competing, deployment.store, genesis_hash, &owner);
    let signature_on_chain = submitter
        .cancel_proxy_transaction(&intent, signature, recovery_id)?
        .send()
        .await?;
    let after = admin.rpc().get_account(&proxy).await?;
    let consumed = evm_proxy::Proxy::try_deserialize(&mut after.data.as_slice())?;
    assert_eq!(consumed.consumed_nonce(), 1);
    let mut expected = before.clone();
    expected.data[48..56].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(after, expected);
    let event: evm_proxy::IntentConsumed =
        super::config_policy::event(admin, &signature_on_chain).await?;
    assert_eq!(
        (event.proxy, event.nonce, event.kind),
        (proxy, 1, evm_proxy::INTENT_KIND_CANCEL)
    );

    let replay = submitter
        .cancel_proxy_transaction(&intent, signature, recovery_id)?
        .send()
        .await
        .expect_err("consumed nonce replay");
    assert_eq!(
        gmsol_sdk::Error::from(replay).anchor_error_code(),
        Some(6006)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, after);

    let canceled = submitter
        .cancel_proxy_transaction(&competing, competing_signature, competing_recovery)?
        .send()
        .await
        .expect_err("another signed message at the canceled nonce");
    assert_eq!(
        gmsol_sdk::Error::from(canceled).anchor_error_code(),
        Some(6006)
    );
    assert_eq!(admin.rpc().get_account(&proxy).await?, after);

    let second_signature = submitter
        .cancel_proxy_transaction(&forged, forged_signature, forged_recovery)?
        .send()
        .await?;
    let mut expected_second = after.clone();
    expected_second.data[48..56].copy_from_slice(&2_u64.to_le_bytes());
    assert_eq!(admin.rpc().get_account(&proxy).await?, expected_second);
    let second_event: evm_proxy::IntentConsumed =
        super::config_policy::event(admin, &second_signature).await?;
    assert_eq!(
        (second_event.proxy, second_event.nonce, second_event.kind),
        (proxy, 2, evm_proxy::INTENT_KIND_CANCEL)
    );

    admin
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::SetPaused { paused: true })
        .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
            admin: admin.payer(),
            config,
        })
        .send()
        .await?;
    admin
        .store_transaction()
        .program(evm_proxy::ID)
        .anchor_args(evm_proxy::instruction::SetChainAllowed {
            chain_id,
            allowed: false,
        })
        .anchor_accounts(evm_proxy::accounts::CurrentConfigAdmin {
            admin: admin.payer(),
            config,
        })
        .send()
        .await?;
    Ok(())
}
