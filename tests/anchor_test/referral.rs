//! Ordinary validator coverage within the singleton Config lifecycle.
use super::setup::Deployment;
use base64::Engine;
use gmsol_evm_proxy as proxy_program;
use gmsol_programs::anchor_lang::{AccountDeserialize, InstructionData};
use gmsol_sdk::{
    client::ops::SystemProgramOps,
    evm_proxy::{set_referral_code_instruction, CancelIntent, SetReferralCodeIntent},
};
use libsecp256k1::{Message, PublicKey, SecretKey};
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_program,
    transaction::Transaction,
};
use solana_transaction_status::{option_serializer::OptionSerializer, UiTransactionEncoding};

async fn snapshot(deployment: &Deployment, keys: &[Pubkey]) -> eyre::Result<Vec<Option<Account>>> {
    let response = deployment
        .client
        .rpc()
        .get_multiple_accounts_with_commitment(keys, CommitmentConfig::confirmed())
        .await?;
    let accounts: Vec<_> = keys.iter().zip(&response.value).map(|(key, account)| {
        serde_json::json!({"key": key.to_string(), "account": account.as_ref().map(|account| {
            serde_json::json!({"owner": account.owner.to_string(), "lamports": account.lamports.to_string(),
                "dataBase64": base64::engine::general_purpose::STANDARD.encode(&account.data),
                "executable": account.executable, "rentEpoch": account.rent_epoch.to_string()})
        })})
    }).collect();
    eprintln!(
        "referral snapshot {}",
        serde_json::json!({"slot": response.context.slot.to_string(), "accounts": accounts})
    );
    Ok(response.value)
}

async fn deadline(deployment: &Deployment) -> eyre::Result<u64> {
    let clock = deployment
        .client
        .rpc()
        .get_account_with_commitment(
            &solana_sdk::sysvar::clock::ID,
            CommitmentConfig::processed(),
        )
        .await?
        .value
        .ok_or_else(|| eyre::eyre!("processed Clock missing"))?;
    let clock: solana_sdk::clock::Clock = bincode::deserialize(&clock.data)?;
    Ok(u64::try_from(clock.unix_timestamp)? + 300)
}

async fn create_proxy(deployment: &Deployment) -> eyre::Result<(SecretKey, [u8; 20], Pubkey)> {
    let owner = SecretKey::random(&mut rand_08::rngs::OsRng);
    create_proxy_for_owner(deployment, owner).await
}

async fn create_proxy_for_owner(
    deployment: &Deployment,
    owner: SecretKey,
) -> eyre::Result<(SecretKey, [u8; 20], Pubkey)> {
    let public = PublicKey::from_secret_key(&owner).serialize();
    let hash = solana_sdk::keccak::hash(&public[1..]);
    let address = hash.to_bytes()[12..].try_into().unwrap();
    let proxy = gmsol_sdk::pda::find_proxy_address(&address, &proxy_program::ID).0;
    deployment
        .client
        .store_transaction()
        .program(proxy_program::ID)
        .anchor_args(proxy_program::instruction::CreateProxy {
            evm_address: address,
        })
        .anchor_accounts(proxy_program::accounts::CreateProxy {
            payer: deployment.client.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    Ok((owner, address, proxy))
}

fn signed_instruction(
    deployment: &Deployment,
    sponsor: Pubkey,
    intent: &SetReferralCodeIntent,
    owner: &SecretKey,
    genesis: &[u8; 32],
) -> eyre::Result<Instruction> {
    let (signature, recovery) = libsecp256k1::sign(
        &Message::parse(&intent.digest(&proxy_program::ID, &deployment.store, genesis)),
        owner,
    );
    Ok(set_referral_code_instruction(
        proxy_program::ID,
        sponsor,
        deployment.store,
        intent,
        signature.serialize(),
        recovery.serialize(),
    )?)
}

/// Submit real transactions, including failures, so fee attribution uses metadata.
async fn execute(
    deployment: &Deployment,
    sponsor: &Keypair,
    fee_payer: &Keypair,
    instructions: &[Instruction],
    succeeds: bool,
) -> eyre::Result<Vec<String>> {
    let rpc = deployment.client.rpc();
    let before = rpc
        .get_account_with_commitment(&fee_payer.pubkey(), CommitmentConfig::confirmed())
        .await?
        .value
        .ok_or_else(|| eyre::eyre!("fee payer missing before transaction"))?;
    let mut transaction = Transaction::new_with_payer(instructions, Some(&fee_payer.pubkey()));
    let sponsor_signs = transaction.message.account_keys
        [..transaction.message.header.num_required_signatures as usize]
        .contains(&sponsor.pubkey());
    let mut signers = vec![fee_payer];
    if sponsor_signs {
        signers.push(sponsor);
    }
    transaction.try_sign(&signers, rpc.get_latest_blockhash().await?)?;
    let signature = transaction.signatures[0];
    let result = rpc
        .send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig {
                skip_preflight: true,
                ..Default::default()
            },
        )
        .await;
    assert_eq!(
        result.is_ok(),
        succeeds,
        "transaction {signature}: {result:?}"
    );
    // Failed transaction confirmation can precede RPC history indexing.
    let mut attempt = 0;
    let observed = loop {
        match rpc
            .get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
                    encoding: Some(UiTransactionEncoding::Json),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            )
            .await
        {
            Ok(observed) => break observed,
            Err(error) if attempt < 19 => {
                attempt += 1;
                eprintln!("referral metadata retry {attempt} for {signature}: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    eprintln!(
        "referral confirmed transaction {}",
        serde_json::to_string(&observed)?
    );
    let meta = observed
        .transaction
        .meta
        .expect("confirmed transaction metadata");
    assert_eq!(meta.err.is_none(), succeeds);
    assert!(meta.fee > 0);
    let after = rpc
        .get_account_with_commitment(&fee_payer.pubkey(), CommitmentConfig::confirmed())
        .await?
        .value
        .ok_or_else(|| eyre::eyre!("fee payer missing after transaction"))?;
    assert_eq!(before.lamports.checked_sub(after.lamports), Some(meta.fee));
    assert_eq!((before.owner, before.data), (after.owner, after.data));
    assert_eq!(meta.pre_balances[0], before.lamports);
    assert_eq!(meta.post_balances[0], after.lamports);
    let logs = match meta.log_messages {
        OptionSerializer::Some(logs) => logs,
        _ => eyre::bail!("transaction logs missing"),
    };
    eprintln!(
        "referral transaction {signature}: success={succeeds}, fee={}, logs={logs:?}",
        meta.fee
    );
    Ok(logs)
}

pub(super) async fn verify_signed_referral(
    deployment: &Deployment,
    genesis: [u8; 32],
) -> eyre::Result<()> {
    let config = gmsol_sdk::pda::find_proxy_config_address(&proxy_program::ID).0;
    assert_policy(deployment, config, true, &[]).await?;
    let mut chain = [0; 32];
    chain[31] = 1;
    policy_chain(deployment, chain, true).await?;
    policy_pause(deployment, false).await?;
    assert_policy(deployment, config, false, &[chain]).await?;
    let result = verify_referral_cases(deployment, genesis, chain).await;
    policy_pause(deployment, true).await?;
    let account = deployment.client.rpc().get_account(&config).await?;
    let state = proxy_program::Config::try_deserialize(&mut account.data.as_slice())?;
    for enabled in state.allowed_chain_ids {
        policy_chain(deployment, enabled, false).await?;
    }
    assert_policy(deployment, config, true, &[]).await?;
    result
}

async fn assert_policy(
    deployment: &Deployment,
    address: Pubkey,
    paused: bool,
    chains: &[[u8; 32]],
) -> eyre::Result<()> {
    let account = deployment.client.rpc().get_account(&address).await?;
    let state = proxy_program::Config::try_deserialize(&mut account.data.as_slice())?;
    eprintln!(
        "referral Config boundary: paused={}, chains={:?}",
        state.paused, state.allowed_chain_ids
    );
    assert_eq!(state.paused, paused);
    assert_eq!(state.allowed_chain_ids, chains);
    Ok(())
}

async fn policy_chain(deployment: &Deployment, chain: [u8; 32], allowed: bool) -> eyre::Result<()> {
    deployment
        .client
        .evm_proxy_instruction(
            gmsol_sdk::client::ops::evm_proxy::set_chain_allowed_instruction(
                proxy_program::ID,
                deployment.client.payer(),
                chain,
                allowed,
            ),
        )
        .send()
        .await?;
    Ok(())
}

async fn policy_pause(deployment: &Deployment, paused: bool) -> eyre::Result<()> {
    deployment
        .client
        .evm_proxy_instruction(gmsol_sdk::client::ops::evm_proxy::set_paused_instruction(
            proxy_program::ID,
            deployment.client.payer(),
            paused,
        ))
        .send()
        .await?;
    Ok(())
}

async fn verify_referral_cases(
    deployment: &Deployment,
    genesis: [u8; 32],
    chain: [u8; 32],
) -> eyre::Result<()> {
    let sponsor = Keypair::new();
    let fee_payer = Keypair::new();
    for key in [sponsor.pubkey(), fee_payer.pubkey()] {
        deployment
            .client
            .transfer(&key, 100_000_000)?
            .send()
            .await?;
    }
    let (owner, address, proxy) = create_proxy(deployment).await?;
    let code = [0, 0, 0, 0, 0, 0, 0, 37];
    let user = gmsol_sdk::pda::find_user_address(&deployment.store, &proxy, &gmsol_store::ID).0;
    let code_account =
        gmsol_sdk::pda::find_referral_code_address(&deployment.store, code, &gmsol_store::ID).0;
    let keys = [proxy, user, code_account, sponsor.pubkey()];
    let intent = SetReferralCodeIntent {
        evm_address: address,
        chain_id: chain,
        code,
        nonce: 1,
        deadline: deadline(deployment).await?,
    };
    let instruction = signed_instruction(deployment, sponsor.pubkey(), &intent, &owner, &genesis)?;
    let before = snapshot(deployment, &keys).await?;
    assert!(before[1].is_none() && before[2].is_none());

    policy_pause(deployment, true).await?;
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[instruction.clone()],
        false,
    )
    .await?;
    assert!(logs.iter().any(|line| line.contains("Paused")));
    policy_pause(deployment, false).await?;
    assert_eq!(snapshot(deployment, &keys).await?, before);
    let future = SetReferralCodeIntent {
        deadline: deadline(deployment).await? + 3600,
        ..intent
    };
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[signed_instruction(
            deployment,
            sponsor.pubkey(),
            &future,
            &owner,
            &genesis,
        )?],
        false,
    )
    .await?;
    assert!(logs.iter().any(|line| line.contains("InvalidDeadline")));
    assert_eq!(snapshot(deployment, &keys).await?, before);
    for (program, store, salt) in [
        (Pubkey::new_unique(), deployment.store, genesis),
        (proxy_program::ID, Pubkey::new_unique(), genesis),
        (proxy_program::ID, deployment.store, [0x59; 32]),
    ] {
        let (sig, recovery) = libsecp256k1::sign(
            &Message::parse(&intent.digest(&program, &store, &salt)),
            &owner,
        );
        let ix = set_referral_code_instruction(
            proxy_program::ID,
            sponsor.pubkey(),
            deployment.store,
            &intent,
            sig.serialize(),
            recovery.serialize(),
        )?;
        let logs = execute(deployment, &sponsor, &fee_payer, &[ix], false).await?;
        assert!(logs.iter().any(|line| line.contains("InvalidSignature")));
        assert_eq!(snapshot(deployment, &keys).await?, before);
    }
    let (signed, recovery) = libsecp256k1::sign(
        &Message::parse(&intent.digest(&proxy_program::ID, &deployment.store, &genesis)),
        &owner,
    );
    let tampered = SetReferralCodeIntent {
        code: 700u64.to_be_bytes(),
        ..intent
    };
    let ix = set_referral_code_instruction(
        proxy_program::ID,
        sponsor.pubkey(),
        deployment.store,
        &tampered,
        signed.serialize(),
        recovery.serialize(),
    )?;
    let logs = execute(deployment, &sponsor, &fee_payer, &[ix], false).await?;
    assert!(logs.iter().any(|line| line.contains("InvalidSignature")));
    assert_eq!(snapshot(deployment, &keys).await?, before);
    for index in [1, 2] {
        let mut wrong = instruction.clone();
        wrong.accounts[index].pubkey = if index == 1 {
            proxy
        } else {
            gmsol_sdk::pda::find_proxy_config_address(&proxy_program::ID).0
        };
        execute(deployment, &sponsor, &fee_payer, &[wrong], false).await?;
        assert_eq!(snapshot(deployment, &keys).await?, before);
    }
    let (_, _, substituted_proxy) = create_proxy(deployment).await?;
    let mut substitution_keys = keys.to_vec();
    substitution_keys.push(substituted_proxy);
    let substitution_before = snapshot(deployment, &substitution_keys).await?;
    let mut wrong_proxy = instruction.clone();
    wrong_proxy.accounts[2].pubkey = substituted_proxy;
    let logs = execute(deployment, &sponsor, &fee_payer, &[wrong_proxy], false).await?;
    assert!(logs.iter().any(|line| line.contains("ConstraintSeeds")));
    assert_eq!(
        snapshot(deployment, &substitution_keys).await?,
        substitution_before
    );
    // The normal Store build forbids non-default Stores. A second valid Store
    // cannot be produced here without changing that deployment policy. The
    // Config has_one binding is reviewed in source; wrong-type input is tested below.

    // The instruction succeeds before a later real System instruction fails.
    let failure = system_instruction::transfer(&sponsor.pubkey(), &Pubkey::new_unique(), u64::MAX);
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[instruction.clone(), failure],
        false,
    )
    .await?;
    assert!(logs
        .iter()
        .any(|line| line == &format!("Program {} success", proxy_program::ID)));
    assert_eq!(
        snapshot(deployment, &keys).await?,
        before,
        "later failure rolls back new User, code, rent and nonce"
    );

    // External privileges must survive message compilation; sponsor is not fee payer.
    let mut unsigned_sponsor = instruction.clone();
    unsigned_sponsor.accounts[0].is_signer = false;
    let logs = execute(deployment, &sponsor, &fee_payer, &[unsigned_sponsor], false).await?;
    assert!(logs.iter().any(|line| line.contains("AccountNotSigner")));
    assert_eq!(snapshot(deployment, &keys).await?, before);
    for index in [0, 2, 4, 5] {
        let mut readonly = instruction.clone();
        readonly.accounts[index].is_writable = false;
        let logs = execute(deployment, &sponsor, &fee_payer, &[readonly], false).await?;
        assert!(logs.iter().any(|line| line.contains("ConstraintMut")));
        assert_eq!(snapshot(deployment, &keys).await?, before);
    }
    for index in [3, 4, 5, 6, 7] {
        let mut wrong = instruction.clone();
        wrong.accounts[index].pubkey = match index {
            4 | 5 => sponsor.pubkey(),
            7 => proxy_program::ID,
            _ => system_program::ID,
        };
        let logs = execute(deployment, &sponsor, &fee_payer, &[wrong], false).await?;
        if matches!(index, 4 | 5) {
            assert!(logs.iter().any(|line| line.contains("ConstraintSeeds")));
        }
        assert_eq!(snapshot(deployment, &keys).await?, before);
    }

    for (invalid, error_name) in [
        (
            SetReferralCodeIntent { nonce: 0, ..intent },
            "InvalidIntentNonce",
        ),
        (
            SetReferralCodeIntent {
                chain_id: [0; 32],
                ..intent
            },
            "ChainNotAllowed",
        ),
        (
            SetReferralCodeIntent {
                deadline: 0,
                ..intent
            },
            "InvalidDeadline",
        ),
    ] {
        let logs = execute(
            deployment,
            &sponsor,
            &fee_payer,
            &[signed_instruction(
                deployment,
                sponsor.pubkey(),
                &invalid,
                &owner,
                &genesis,
            )?],
            false,
        )
        .await?;
        assert!(
            logs.iter().any(|line| line.contains(error_name)),
            "expected {error_name}"
        );
        assert_eq!(snapshot(deployment, &keys).await?, before);
    }
    let wrong_owner = SecretKey::random(&mut rand_08::rngs::OsRng);
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[signed_instruction(
            deployment,
            sponsor.pubkey(),
            &intent,
            &wrong_owner,
            &genesis,
        )?],
        false,
    )
    .await?;
    assert!(logs.iter().any(|line| line.contains("InvalidSignature")));
    assert_eq!(snapshot(deployment, &keys).await?, before);

    // Signing an untrimmed alias verbatim does not sign the canonical code.
    let typed = intent.typed_data(&proxy_program::ID, &deployment.store, &genesis);
    let word = |value: u64| {
        let mut bytes = [0; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        bytes
    };
    let structure = solana_sdk::keccak::hashv(&[
        solana_sdk::keccak::hash(typed.type_signature.as_bytes()).as_ref(),
        proxy.as_ref(),
        solana_sdk::keccak::hash(format!("1{}", typed.code).as_bytes()).as_ref(),
        &word(intent.nonce),
        &word(intent.deadline),
    ])
    .to_bytes();
    let domain = solana_sdk::keccak::hashv(&[
        solana_sdk::keccak::hash(
            b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)",
        )
        .as_ref(),
        solana_sdk::keccak::hash(typed.name.as_bytes()).as_ref(),
        solana_sdk::keccak::hash(typed.version.as_bytes()).as_ref(),
        &chain,
        &typed.salt,
    ]);
    let alias_digest =
        solana_sdk::keccak::hashv(&[b"\x19\x01", domain.as_ref(), &structure]).to_bytes();
    assert_ne!(
        alias_digest,
        intent.digest(&proxy_program::ID, &deployment.store, &genesis)
    );
    let (alias_signature, alias_recovery) =
        libsecp256k1::sign(&Message::parse(&alias_digest), &owner);
    let mut alias = instruction.clone();
    alias.data = proxy_program::instruction::SetReferralCode {
        evm_address: address,
        chain_id: chain,
        code,
        nonce: intent.nonce,
        deadline: intent.deadline,
        signature: alias_signature.serialize(),
        recovery_id: alias_recovery.serialize(),
    }
    .data();
    let logs = execute(deployment, &sponsor, &fee_payer, &[alias], false).await?;
    assert!(logs.iter().any(|line| line.contains("InvalidSignature")));
    assert_eq!(snapshot(deployment, &keys).await?, before);

    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[instruction.clone()],
        true,
    )
    .await?;
    for name in ["PrepareUser", "InitializeReferralCode"] {
        assert!(
            logs.iter()
                .any(|line| line == &format!("Program log: Instruction: {name}")),
            "missing reached CPI {name}"
        );
    }
    let after = snapshot(deployment, &keys).await?;
    let registered_user = after[1].as_ref().unwrap();
    let registered_code = after[2].as_ref().unwrap();
    assert_eq!(
        (registered_user.owner, registered_code.owner),
        (gmsol_store::ID, gmsol_store::ID)
    );
    assert_eq!(
        registered_user.data.len(),
        8 + gmsol_store::states::UserHeader::space(0)
    );
    let decoded_user = gmsol_sdk::utils::zero_copy::try_deserialize::<
        gmsol_store::states::UserHeader,
    >(&registered_user.data)?;
    assert!(decoded_user.is_initialized());
    assert_eq!(decoded_user.referral().code(), Some(&code_account));
    let decoded_code = gmsol_store::states::user::ReferralCodeV2::try_deserialize(
        &mut registered_code.data.as_slice(),
    )?;
    assert_eq!(
        (decoded_code.code, decoded_code.owner, decoded_code.store),
        (code, proxy, deployment.store)
    );
    assert_eq!(
        before[0].as_ref().unwrap().lamports,
        after[0].as_ref().unwrap().lamports
    );
    assert_eq!(
        before[3].as_ref().unwrap().lamports - after[3].as_ref().unwrap().lamports,
        registered_user.lamports + registered_code.lamports
    );
    let proxy_data =
        proxy_program::Proxy::try_deserialize(&mut after[0].as_ref().unwrap().data.as_slice())?;
    assert_eq!(proxy_data.consumed_nonce(), 1);
    let events = logs
        .iter()
        .filter_map(|line| gmsol_sdk::evm_proxy::decode_referral_code_event_log(line).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].proxy, events[0].nonce, events[0].code),
        (proxy, 1, code)
    );
    let consumed = logs
        .iter()
        .filter_map(|line| gmsol_sdk::evm_proxy::decode_intent_event_log(line).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(consumed.len(), 1);
    assert_eq!(
        (consumed[0].proxy, consumed[0].nonce, consumed[0].kind),
        (proxy, 1, proxy_program::INTENT_KIND_SET_REFERRAL_CODE)
    );

    execute(deployment, &sponsor, &fee_payer, &[instruction], false).await?;
    assert_eq!(
        snapshot(deployment, &keys).await?,
        after,
        "replay cannot change business state"
    );

    // An existing coded User reaches idempotent preparation, then fails the second CPI.
    let unused_code = [0, 0, 0, 0, 0, 0, 0, 38];
    let unused_account = gmsol_sdk::pda::find_referral_code_address(
        &deployment.store,
        unused_code,
        &gmsol_store::ID,
    )
    .0;
    let existing_keys = [proxy, user, code_account, unused_account, sponsor.pubkey()];
    let existing_before = snapshot(deployment, &existing_keys).await?;
    let next = SetReferralCodeIntent {
        code: unused_code,
        nonce: 2,
        deadline: deadline(deployment).await?,
        ..intent
    };
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[signed_instruction(
            deployment,
            sponsor.pubkey(),
            &next,
            &owner,
            &genesis,
        )?],
        false,
    )
    .await?;
    assert!(logs
        .iter()
        .any(|line| line == "Program log: Instruction: PrepareUser"));
    assert!(logs
        .iter()
        .any(|line| line == "Program log: Instruction: InitializeReferralCode"));
    assert!(logs
        .iter()
        .any(|line| line.contains("ReferralCodeHasBeenSet")));
    assert_eq!(
        logs.iter()
            .filter(|line| *line == &format!("Program {} success", gmsol_store::ID))
            .count(),
        1
    );
    assert_eq!(snapshot(deployment, &existing_keys).await?, existing_before);

    // Another proxy cannot claim an occupied code; this is prevalidation, not CPI rollback.
    let (other_owner, other_address, other_proxy) = create_proxy(deployment).await?;
    let occupied = SetReferralCodeIntent {
        evm_address: other_address,
        deadline: deadline(deployment).await?,
        ..intent
    };
    let other_user =
        gmsol_sdk::pda::find_user_address(&deployment.store, &other_proxy, &gmsol_store::ID).0;
    let occupied_keys = [other_proxy, other_user, code_account, sponsor.pubkey()];
    let occupied_before = snapshot(deployment, &occupied_keys).await?;
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[signed_instruction(
            deployment,
            sponsor.pubkey(),
            &occupied,
            &other_owner,
            &genesis,
        )?],
        false,
    )
    .await?;
    assert!(!logs
        .iter()
        .any(|line| line == "Program log: Instruction: PrepareUser"));
    assert_eq!(snapshot(deployment, &occupied_keys).await?, occupied_before);

    // Exercise each target's prefunding boundary in the same transaction. Tiny
    // System balances need not survive a separate transaction's rent checks.
    let user_rent = deployment
        .client
        .rpc()
        .get_minimum_balance_for_rent_exemption(8 + gmsol_store::states::UserHeader::space(0))
        .await?;
    let code_rent = deployment
        .client
        .rpc()
        .get_minimum_balance_for_rent_exemption(
            8 + std::mem::size_of::<gmsol_store::states::user::ReferralCodeV2>(),
        )
        .await?;
    for (case, (user_balance, code_balance)) in [
        (0, 0),
        (1, 1),
        (user_rent - 1, code_rent - 1),
        (user_rent, code_rent),
        (user_rent + 77, code_rent + 77),
    ]
    .into_iter()
    .enumerate()
    {
        let (funded_owner, funded_address, funded_proxy) = create_proxy(deployment).await?;
        let funded_code = (100 + case as u64).to_be_bytes();
        let funded_user =
            gmsol_sdk::pda::find_user_address(&deployment.store, &funded_proxy, &gmsol_store::ID).0;
        let funded_code_account = gmsol_sdk::pda::find_referral_code_address(
            &deployment.store,
            funded_code,
            &gmsol_store::ID,
        )
        .0;
        let funded_keys = [
            funded_proxy,
            funded_user,
            funded_code_account,
            sponsor.pubkey(),
        ];
        let funded_before = snapshot(deployment, &funded_keys).await?;
        let funded_intent = SetReferralCodeIntent {
            evm_address: funded_address,
            code: funded_code,
            deadline: deadline(deployment).await?,
            ..intent
        };
        let mut instructions = Vec::new();
        for (target, balance) in [
            (funded_user, user_balance),
            (funded_code_account, code_balance),
        ] {
            if balance != 0 {
                instructions.push(system_instruction::transfer(
                    &sponsor.pubkey(),
                    &target,
                    balance,
                ));
            }
        }
        instructions.push(signed_instruction(
            deployment,
            sponsor.pubkey(),
            &funded_intent,
            &funded_owner,
            &genesis,
        )?);
        execute(deployment, &sponsor, &fee_payer, &instructions, true).await?;
        let funded_after = snapshot(deployment, &funded_keys).await?;
        assert_eq!(
            funded_after[1].as_ref().unwrap().lamports,
            user_rent.max(user_balance)
        );
        assert_eq!(
            funded_after[2].as_ref().unwrap().lamports,
            code_rent.max(code_balance)
        );
        assert_eq!(
            funded_before[0].as_ref().unwrap().lamports,
            funded_after[0].as_ref().unwrap().lamports
        );
        assert_eq!(
            funded_before[3].as_ref().unwrap().lamports
                - funded_after[3].as_ref().unwrap().lamports,
            user_rent.max(user_balance) + code_rent.max(code_balance)
        );
    }

    // Persistent balances must survive a rejected registration and reduce only
    // the corresponding deficit on success. Tiny balances stay in atomic cases above.
    let mut alternate_chain = [0; 32];
    alternate_chain[31] = 2;
    policy_chain(deployment, alternate_chain, true).await?;
    for (case, (user_balance, code_balance)) in [
        (user_rent + 77, 0),
        (0, code_rent + 77),
        (user_rent, code_rent),
    ]
    .into_iter()
    .enumerate()
    {
        let (persistent_owner, persistent_address, persistent_proxy) =
            create_proxy(deployment).await?;
        let persistent_code = (400 + case as u64).to_be_bytes();
        let persistent_user = gmsol_sdk::pda::find_user_address(
            &deployment.store,
            &persistent_proxy,
            &gmsol_store::ID,
        )
        .0;
        let persistent_account = gmsol_sdk::pda::find_referral_code_address(
            &deployment.store,
            persistent_code,
            &gmsol_store::ID,
        )
        .0;
        let mut prefunds = Vec::new();
        for (target, amount) in [
            (persistent_user, user_balance),
            (persistent_account, code_balance),
        ] {
            if amount != 0 {
                prefunds.push(system_instruction::transfer(
                    &sponsor.pubkey(),
                    &target,
                    amount,
                ));
            }
        }
        execute(deployment, &sponsor, &fee_payer, &prefunds, true).await?;
        let persistent_keys = [
            persistent_proxy,
            persistent_user,
            persistent_account,
            sponsor.pubkey(),
        ];
        let prior = snapshot(deployment, &persistent_keys).await?;
        let persistent = SetReferralCodeIntent {
            evm_address: persistent_address,
            chain_id: alternate_chain,
            code: persistent_code,
            deadline: deadline(deployment).await?,
            ..intent
        };
        let bad = SetReferralCodeIntent {
            nonce: 2,
            ..persistent
        };
        let logs = execute(
            deployment,
            &sponsor,
            &fee_payer,
            &[signed_instruction(
                deployment,
                sponsor.pubkey(),
                &bad,
                &persistent_owner,
                &genesis,
            )?],
            false,
        )
        .await?;
        assert!(logs.iter().any(|line| line.contains("InvalidIntentNonce")));
        assert_eq!(snapshot(deployment, &persistent_keys).await?, prior);
        let persistent_ix = signed_instruction(
            deployment,
            sponsor.pubkey(),
            &persistent,
            &persistent_owner,
            &genesis,
        )?;
        let logs = execute(
            deployment,
            &sponsor,
            &fee_payer,
            &[
                persistent_ix.clone(),
                system_instruction::transfer(&sponsor.pubkey(), &Pubkey::new_unique(), u64::MAX),
            ],
            false,
        )
        .await?;
        assert!(logs
            .iter()
            .any(|line| line == &format!("Program {} success", proxy_program::ID)));
        assert_eq!(
            snapshot(deployment, &persistent_keys).await?,
            prior,
            "later failure must preserve prior successful prefunds"
        );
        execute(deployment, &sponsor, &fee_payer, &[persistent_ix], true).await?;
        let after = snapshot(deployment, &persistent_keys).await?;
        assert_eq!(
            after[1].as_ref().unwrap().lamports,
            user_rent.max(user_balance)
        );
        assert_eq!(
            after[2].as_ref().unwrap().lamports,
            code_rent.max(code_balance)
        );
        assert_eq!(
            prior[3].as_ref().unwrap().lamports - after[3].as_ref().unwrap().lamports,
            user_rent.saturating_sub(user_balance) + code_rent.saturating_sub(code_balance)
        );
        assert_eq!(
            prior[0].as_ref().unwrap().lamports,
            after[0].as_ref().unwrap().lamports
        );
    }

    // Explicit standard-validator genesis fixture, not a production creation claim.
    let fixture_owner = SecretKey::parse(&[0x36; 32])?;
    let (fixture_owner, fixture_address, fixture_proxy) =
        create_proxy_for_owner(deployment, fixture_owner).await?;
    assert_eq!(
        deployment.store,
        deployment.client.find_store_address(""),
        "genesis User fixture requires the standard default Store"
    );
    let fixture_user =
        gmsol_sdk::pda::find_user_address(&deployment.store, &fixture_proxy, &gmsol_store::ID).0;
    let fixture_code = 500u64.to_be_bytes();
    let fixture_code_account = gmsol_sdk::pda::find_referral_code_address(
        &deployment.store,
        fixture_code,
        &gmsol_store::ID,
    )
    .0;
    let fixture_keys = [
        fixture_proxy,
        fixture_user,
        fixture_code_account,
        sponsor.pubkey(),
    ];
    let prior = snapshot(deployment, &fixture_keys).await?;
    let existing = prior[1]
        .as_ref()
        .expect("standard validator must preload uncoded User fixture");
    assert_eq!(existing.owner, gmsol_store::ID);
    assert_eq!(
        existing.data.len(),
        8 + gmsol_store::states::UserHeader::space(0)
    );
    let decoded = gmsol_sdk::utils::zero_copy::try_deserialize::<gmsol_store::states::UserHeader>(
        &existing.data,
    )?;
    assert!(decoded.is_initialized());
    assert!(decoded.referral().code().is_none());
    let fixture_intent = SetReferralCodeIntent {
        evm_address: fixture_address,
        code: fixture_code,
        deadline: deadline(deployment).await?,
        ..intent
    };
    let logs = execute(
        deployment,
        &sponsor,
        &fee_payer,
        &[signed_instruction(
            deployment,
            sponsor.pubkey(),
            &fixture_intent,
            &fixture_owner,
            &genesis,
        )?],
        true,
    )
    .await?;
    assert!(logs
        .iter()
        .any(|line| line == "Program log: Instruction: PrepareUser"));
    assert!(logs
        .iter()
        .any(|line| line == "Program log: Instruction: InitializeReferralCode"));
    let after = snapshot(deployment, &fixture_keys).await?;
    let next = after[1].as_ref().unwrap();
    assert_eq!(next.lamports, existing.lamports);
    // Header 80-byte prefix, then Referral's referrer and code keys.
    let code_offset = 8 + 80 + 32;
    let mut expected = existing.data.clone();
    expected[code_offset..code_offset + 32].copy_from_slice(fixture_code_account.as_ref());
    assert_eq!(
        next.data, expected,
        "unrelated User bytes must survive idempotent preparation"
    );
    assert_eq!(
        prior[3].as_ref().unwrap().lamports - after[3].as_ref().unwrap().lamports,
        code_rent
    );
    assert_eq!(
        prior[0].as_ref().unwrap().lamports,
        after[0].as_ref().unwrap().lamports
    );

    // Sponsor shortfall at the first and second transfer must roll everything back.
    for balance in [1, user_rent + 1] {
        let poor_sponsor = Keypair::new();
        // Rent-exempt sponsor, then drain within the transaction to the chosen balance.
        deployment
            .client
            .transfer(&poor_sponsor.pubkey(), user_rent + code_rent)?
            .send()
            .await?;
        let (poor_owner, poor_address, poor_proxy) = create_proxy(deployment).await?;
        let poor_code = (200 + balance).to_be_bytes();
        let poor_user =
            gmsol_sdk::pda::find_user_address(&deployment.store, &poor_proxy, &gmsol_store::ID).0;
        let poor_code_account = gmsol_sdk::pda::find_referral_code_address(
            &deployment.store,
            poor_code,
            &gmsol_store::ID,
        )
        .0;
        let drain_recipient = Pubkey::new_unique();
        let poor_keys = [
            poor_proxy,
            poor_user,
            poor_code_account,
            poor_sponsor.pubkey(),
            drain_recipient,
        ];
        let poor_before = snapshot(deployment, &poor_keys).await?;
        let poor_intent = SetReferralCodeIntent {
            evm_address: poor_address,
            code: poor_code,
            deadline: deadline(deployment).await?,
            ..intent
        };
        let drain = system_instruction::transfer(
            &poor_sponsor.pubkey(),
            &drain_recipient,
            user_rent + code_rent - balance,
        );
        let logs = execute(
            deployment,
            &poor_sponsor,
            &fee_payer,
            &[
                drain,
                signed_instruction(
                    deployment,
                    poor_sponsor.pubkey(),
                    &poor_intent,
                    &poor_owner,
                    &genesis,
                )?,
            ],
            false,
        )
        .await?;
        assert!(logs
            .iter()
            .any(|line| line.contains("insufficient lamports")));
        assert!(logs
            .iter()
            .any(|line| line == &format!("Program {} invoke [1]", proxy_program::ID)));
        assert_eq!(
            logs.iter()
                .filter(|line| *line == &format!("Program {} success", system_program::ID))
                .count(),
            if balance == 1 { 1 } else { 2 }
        );
        assert!(!logs
            .iter()
            .any(|line| line == "Program log: Instruction: PrepareUser"));
        assert_eq!(snapshot(deployment, &poor_keys).await?, poor_before);
    }

    // The public builder rejects zero, but direct instruction callers must also fail.
    let mut zero = signed_instruction(
        deployment,
        sponsor.pubkey(),
        &occupied,
        &other_owner,
        &genesis,
    )?;
    zero.accounts[5].pubkey =
        gmsol_sdk::pda::find_referral_code_address(&deployment.store, [0; 8], &gmsol_store::ID).0;
    let mut zero_keys = occupied_keys.to_vec();
    zero_keys.push(zero.accounts[5].pubkey);
    let zero_before = snapshot(deployment, &zero_keys).await?;
    zero.data = proxy_program::instruction::SetReferralCode {
        evm_address: other_address,
        chain_id: chain,
        code: [0; 8],
        nonce: 1,
        deadline: deadline(deployment).await?,
        signature: [1; 64],
        recovery_id: 0,
    }
    .data();
    let logs = execute(deployment, &sponsor, &fee_payer, &[zero], false).await?;
    assert!(logs.iter().any(|line| line.contains("InvalidReferralCode")));
    assert_eq!(snapshot(deployment, &zero_keys).await?, zero_before);

    // Normal-range cross-intent sequence: registration 1, Cancel 2, Withdraw 3.
    let cancel = CancelIntent {
        evm_address: address,
        chain_id: alternate_chain,
        nonce: 2,
        deadline: deadline(deployment).await?,
    };
    let (signature, recovery) = libsecp256k1::sign(
        &Message::parse(&cancel.digest(&proxy_program::ID, &deployment.store, &genesis)),
        &owner,
    );
    let cancel_ix = gmsol_sdk::evm_proxy::cancel_instruction(
        proxy_program::ID,
        sponsor.pubkey(),
        &cancel,
        signature.serialize(),
        recovery.serialize(),
    )?;
    execute(deployment, &sponsor, &fee_payer, &[cancel_ix], true).await?;
    policy_chain(deployment, alternate_chain, false).await?;
    super::withdraw::verify_signed_withdraw(
        deployment,
        &owner,
        address,
        proxy,
        chain,
        genesis,
        deadline(deployment).await?,
    )
    .await?;
    Ok(())
}
