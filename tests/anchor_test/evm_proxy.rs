use gmsol_evm_proxy::{
    accounts, instruction, IntentPair, Proxy, ProxyError, ID, INTENT_KIND_WITHDRAW, PROXY_VERSION,
};
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, Discriminator};
use gmsol_sdk::{
    client::ops::{
        evm_proxy::{
            create_config_instruction, set_chain_allowed_instruction, set_paused_instruction,
        },
        SystemProgramOps,
    },
    evm_proxy::{decode_intent_event_log, decode_withdraw_event_log, WithdrawIntent},
    pda::find_proxy_address,
};
use gmsol_solana_utils::signer::shared_signer;
use solana_sdk::{
    instruction::InstructionError,
    keccak,
    signature::{Keypair, Signer},
    system_program,
    transaction::TransactionError,
};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::anchor_test::setup::current_deployment;

#[tokio::test]
async fn initialized_proxy_withdraw_on_validator_moves_exact_tokens_and_consumes_pair(
) -> eyre::Result<()> {
    use anchor_spl::{associated_token::spl_associated_token_account, token::spl_token};
    use libsecp256k1::{Message, PublicKey, SecretKey};

    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let admin = deployment.client.payer();
    let genesis = deployment.client.rpc().get_genesis_hash().await?.to_bytes();
    let mut chain = [0; 32];
    chain[31] = 1;
    let signer_key = Keypair::new();
    let secret = SecretKey::parse_slice(&signer_key.to_bytes()[..32])
        .map_err(|error| eyre::eyre!("invalid in-memory signer key: {error:?}"))?;
    let public = PublicKey::from_secret_key(&secret).serialize();
    let hash = keccak::hash(&public[1..]).to_bytes();
    let evm_address: [u8; 20] = hash[12..].try_into()?;
    let (proxy, _) = find_proxy_address(&evm_address, &ID);
    let payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    deployment
        .client
        .transfer(&payer.payer(), 50_000_000)?
        .send()
        .await?;
    deployment
        .client
        .evm_proxy_instruction(create_config_instruction(
            ID,
            admin,
            admin,
            deployment.store,
            genesis,
            admin,
        ))
        .send()
        .await?;
    deployment
        .client
        .evm_proxy_instruction(set_chain_allowed_instruction(ID, admin, chain, true))
        .send()
        .await?;
    deployment
        .client
        .evm_proxy_instruction(set_paused_instruction(ID, admin, false))
        .send()
        .await?;
    payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy { evm_address })
        .anchor_accounts(accounts::CreateProxy {
            payer: payer.payer(),
            proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    let recipient = Keypair::new().pubkey();
    let (mint, source) = deployment.create_unregistered_mint(&proxy).await?;
    let destination = spl_associated_token_account::get_associated_token_address(&recipient, &mint);
    deployment
        .client
        .store_transaction()
        .pre_instruction(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &admin,
                &recipient,
                &mint,
                &spl_token::ID,
            ),
            true,
        )
        .pre_instruction(
            spl_token::instruction::mint_to_checked(
                &spl_token::ID,
                &mint,
                &source,
                &admin,
                &[],
                500,
                6,
            )?,
            true,
        )
        .send()
        .await?;
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(500)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(0)
    );

    let intent = WithdrawIntent {
        evm_address,
        chain_id: chain,
        mint,
        amount: 125,
        decimals: 6,
        recipient,
        generation: 0,
        nonce: 1,
        deadline: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 600,
    };
    let digest = intent.digest(&ID, &deployment.store, &genesis);
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), &secret);
    let signature = signature.serialize();
    let recovery = recovery.serialize();
    let withdraw = payer.withdraw_proxy_transaction(&intent, signature, recovery)?;
    let signed = withdraw
        .signed_transaction_with_options(false, None, None, |_| Ok(()))
        .await?;
    let simulation = payer.rpc().simulate_transaction(&signed).await?.value;
    assert!(
        simulation.err.is_none(),
        "withdraw simulation failed: {:?}",
        simulation.err
    );
    let logs = simulation
        .logs
        .ok_or_else(|| eyre::eyre!("withdraw logs missing"))?;
    withdraw.send().await?;
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(375)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(125)
    );
    let account = payer.rpc().get_account(&proxy).await?;
    let state = Proxy::try_deserialize(&mut account.data.as_slice())?;
    assert_eq!(
        state.consumed_pair(),
        IntentPair {
            generation: 0,
            nonce: 1
        }
    );
    let mut consumed = None;
    let mut withdrawn = None;
    for line in &logs {
        if let Some(event) = decode_intent_event_log(line)? {
            consumed = Some(event);
        }
        if let Some(event) = decode_withdraw_event_log(line)? {
            withdrawn = Some(event);
        }
    }
    let consumed = consumed.ok_or_else(|| eyre::eyre!("intent event missing"))?;
    assert_eq!(
        (consumed.proxy, consumed.generation, consumed.nonce),
        (proxy, 0, 1)
    );
    assert_eq!(consumed.kind, INTENT_KIND_WITHDRAW);
    let withdrawn = withdrawn.ok_or_else(|| eyre::eyre!("withdraw event missing"))?;
    assert_eq!(
        (
            withdrawn.proxy,
            withdrawn.mint,
            withdrawn.amount,
            withdrawn.recipient
        ),
        (proxy, mint, 125, recipient)
    );

    payer
        .withdraw_proxy_transaction(&intent, signature, recovery)?
        .send()
        .await
        .expect_err("a consumed pair cannot be replayed");
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(375)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(125)
    );
    let after = payer.rpc().get_account(&proxy).await?;
    assert_eq!(after.data, account.data);

    let missing_recipient = Keypair::new().pubkey();
    let missing_ata = WithdrawIntent {
        recipient: missing_recipient,
        nonce: 2,
        ..intent
    };
    let digest = missing_ata.digest(&ID, &deployment.store, &genesis);
    let (missing_signature, missing_recovery) =
        libsecp256k1::sign(&Message::parse(&digest), &secret);
    payer
        .withdraw_proxy_transaction(
            &missing_ata,
            missing_signature.serialize(),
            missing_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("a missing destination ATA must fail");
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(375)
    );
    assert_eq!(payer.rpc().get_account(&proxy).await?.data, account.data);

    let next_intent = WithdrawIntent { nonce: 2, ..intent };
    let next_digest = next_intent.digest(&ID, &deployment.store, &genesis);
    let (next_signature, next_recovery) =
        libsecp256k1::sign(&Message::parse(&next_digest), &secret);
    let changed_amount = WithdrawIntent {
        amount: 126,
        ..next_intent
    };
    let err = payer
        .withdraw_proxy_transaction(
            &changed_amount,
            next_signature.serialize(),
            next_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("a changed amount invalidates the signed intent");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::InvalidSignature as u32).into())
    );
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(375)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(125)
    );
    assert_eq!(payer.rpc().get_account(&proxy).await?.data, account.data);
    Ok(())
}

#[tokio::test]
async fn proxy_creation_on_validator_preserves_identity_and_rejects_invalid_requests(
) -> eyre::Result<()> {
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

    let first_address = [0x23; 20];
    let second_address = [0x47; 20];
    let (first_proxy, first_bump) = find_proxy_address(&first_address, &ID);
    let (second_proxy, second_bump) = find_proxy_address(&second_address, &ID);
    assert_ne!(first_proxy, second_proxy);
    assert!(first_payer.rpc().get_account(&first_proxy).await.is_err());

    first_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: first_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;

    let first_account = first_payer.rpc().get_account(&first_proxy).await?;
    assert_eq!(first_account.owner, ID);
    assert_eq!(first_account.data.len(), 8 + std::mem::size_of::<Proxy>());
    assert_eq!(&first_account.data[..8], Proxy::DISCRIMINATOR);
    let first_state = Proxy::try_deserialize(&mut first_account.data.as_slice())?;
    assert_eq!(first_state.version, PROXY_VERSION);
    assert_eq!(first_state.bump, first_bump);
    assert_eq!(first_state.evm_address, first_address);
    assert_eq!(first_state.reserved, [0; 256]);

    second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: first_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: first_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );

    let wrong_proxy = first_proxy;
    let wrong_address = second_address;
    let err = second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: wrong_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: wrong_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a different address cannot reuse an existing proxy PDA");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    assert_eq!(
        second_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );
    assert!(second_payer.rpc().get_account(&second_proxy).await.is_err());

    second_payer
        .store_transaction()
        .program(ID)
        .anchor_args(instruction::CreateProxy {
            evm_address: second_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: second_payer.payer(),
            proxy: second_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await?;
    let second_account = second_payer.rpc().get_account(&second_proxy).await?;
    let second_state = Proxy::try_deserialize(&mut second_account.data.as_slice())?;
    assert_eq!(second_state.version, PROXY_VERSION);
    assert_eq!(second_state.bump, second_bump);
    assert_eq!(second_state.evm_address, second_address);
    assert_eq!(second_state.reserved, [0; 256]);
    assert_eq!(
        first_payer.rpc().get_account(&first_proxy).await?,
        first_account
    );

    let rent_payer = Keypair::new();
    let empty_account_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(0)
        .await?;
    let proxy_rent = first_payer
        .rpc()
        .get_minimum_balance_for_rent_exemption(8 + std::mem::size_of::<Proxy>())
        .await?;
    assert!(empty_account_rent < proxy_rent);
    deployment
        .client
        .transfer(&rent_payer.pubkey(), empty_account_rent)?
        .send()
        .await?;
    assert_eq!(
        first_payer.rpc().get_balance(&rent_payer.pubkey()).await?,
        empty_account_rent
    );
    let third_address = [0x69; 20];
    let (third_proxy, _) = find_proxy_address(&third_address, &ID);
    let err = first_payer
        .store_transaction()
        .program(ID)
        .signer(&rent_payer)
        .anchor_args(instruction::CreateProxy {
            evm_address: third_address,
        })
        .anchor_accounts(accounts::CreateProxy {
            payer: rent_payer.pubkey(),
            proxy: third_proxy,
            system_program: system_program::ID,
        })
        .send()
        .await
        .expect_err("a signer funded for only an empty account cannot fund proxy rent");
    let transaction_error = match &err {
        gmsol_solana_utils::Error::Client(client_error) => client_error.get_transaction_error(),
        _ => panic!("unexpected insufficient-rent error: {err:?}"),
    };
    assert!(
        matches!(
            transaction_error,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::Custom(1)
            ))
        ),
        "unexpected insufficient-rent error: {err:?}"
    );
    assert_eq!(
        first_payer.rpc().get_balance(&rent_payer.pubkey()).await?,
        empty_account_rent
    );
    assert!(first_payer.rpc().get_account(&third_proxy).await.is_err());
    Ok(())
}
