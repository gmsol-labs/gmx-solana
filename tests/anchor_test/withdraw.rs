use super::setup::Deployment;
use gmsol_evm_proxy as evm_proxy;
use gmsol_evm_proxy::{Proxy, ProxyError, ID, INTENT_KIND_WITHDRAW};
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, InstructionData};
use gmsol_sdk::client::ops::SystemProgramOps;
use gmsol_sdk::evm_proxy::CancelIntent;
use gmsol_sdk::evm_proxy::WithdrawIntent;
use gmsol_solana_utils::signer::shared_signer;
use libsecp256k1::{Message, SecretKey};
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::InstructionError;
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::{system_instruction, system_program, transaction::TransactionError};

async fn fresh_deadline(deployment: &Deployment) -> eyre::Result<u64> {
    let account = deployment
        .client
        .rpc()
        .get_account_with_commitment(
            &solana_sdk::sysvar::clock::ID,
            CommitmentConfig::processed(),
        )
        .await?
        .value
        .ok_or_else(|| eyre::eyre!("validator Clock account missing"))?;
    let clock: solana_sdk::clock::Clock = bincode::deserialize(&account.data)?;
    Ok(u64::try_from(clock.unix_timestamp)?
        .checked_add(300)
        .expect("deadline overflow"))
}

async fn assert_unchanged(
    deployment: &Deployment,
    proxy: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    expected: &(Account, Account, Account),
) -> eyre::Result<()> {
    let rpc = deployment.client.rpc();
    assert_eq!(rpc.get_account(proxy).await?, expected.0);
    assert_eq!(rpc.get_account(source).await?, expected.1);
    assert_eq!(rpc.get_account(destination).await?, expected.2);
    Ok(())
}

async fn snapshot(
    deployment: &Deployment,
    proxy: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
) -> eyre::Result<(Account, Account, Account)> {
    let rpc = deployment.client.rpc();
    Ok((
        rpc.get_account(proxy).await?,
        rpc.get_account(source).await?,
        rpc.get_account(destination).await?,
    ))
}

fn assert_token_error(error: gmsol_solana_utils::Error, expected: u32) {
    let transaction_error = match &error {
        gmsol_solana_utils::Error::Client(client_error) => client_error.get_transaction_error(),
        _ => panic!("expected a transaction error, got {error:?}"),
    };
    assert!(
        matches!(
            transaction_error,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::Custom(code)
            )) if code == expected
        ),
        "unexpected token transaction error: {error:?}"
    );
}

fn old_pair_signature(
    intent: &WithdrawIntent,
    store: Pubkey,
    genesis: [u8; 32],
    owner: &SecretKey,
) -> ([u8; 64], u8) {
    use solana_sdk::keccak;

    let typed = intent.typed_data(&ID, &store, &genesis);
    let word = |value: u64| {
        let mut bytes = [0; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        bytes
    };
    let structure = keccak::hashv(&[
        keccak::hash(b"Withdraw(bytes32 proxyAccount,string mint,uint64 amount,uint8 decimals,string recipient,uint64 generation,uint64 nonce,uint64 deadline)").as_ref(),
        typed.proxy_account.as_ref(),
        keccak::hash(typed.mint.as_bytes()).as_ref(),
        &word(intent.amount),
        &word(u64::from(intent.decimals)),
        keccak::hash(typed.recipient.as_bytes()).as_ref(),
        &[0; 32],
        &word(intent.nonce),
        &word(intent.deadline),
    ]);
    let domain = keccak::hashv(&[
        keccak::hash(b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)")
            .as_ref(),
        keccak::hash(typed.name.as_bytes()).as_ref(),
        keccak::hash(typed.version.as_bytes()).as_ref(),
        &typed.chain_id,
        &typed.salt,
    ]);
    let digest = keccak::hashv(&[b"\x19\x01", domain.as_ref(), structure.as_ref()]);
    assert_ne!(digest.to_bytes(), intent.digest(&ID, &store, &genesis));
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest.to_bytes()), owner);
    (signature.serialize(), recovery.serialize())
}

async fn create_freezable_mint(
    deployment: &Deployment,
    owner: &Pubkey,
) -> eyre::Result<(Pubkey, Pubkey)> {
    use anchor_spl::{
        associated_token::spl_associated_token_account,
        token::{spl_token, Mint},
    };
    let mint = Keypair::new();
    let admin = deployment.client.payer();
    let rent = deployment
        .client
        .rpc()
        .get_minimum_balance_for_rent_exemption(Mint::LEN)
        .await?;
    deployment
        .client
        .store_transaction()
        .signer(&mint)
        .pre_instruction(
            system_instruction::create_account(
                &admin,
                &mint.pubkey(),
                rent,
                Mint::LEN as u64,
                &spl_token::ID,
            ),
            true,
        )
        .pre_instruction(
            spl_token::instruction::initialize_mint2(
                &spl_token::ID,
                &mint.pubkey(),
                &admin,
                Some(&admin),
                6,
            )?,
            true,
        )
        .pre_instruction(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &admin,
                owner,
                &mint.pubkey(),
                &spl_token::ID,
            ),
            true,
        )
        .send()
        .await?;
    Ok((
        mint.pubkey(),
        spl_associated_token_account::get_associated_token_address(owner, &mint.pubkey()),
    ))
}

async fn create_token_2022_mint(deployment: &Deployment) -> eyre::Result<Pubkey> {
    use anchor_spl::token_2022::spl_token_2022;
    let mint = Keypair::new();
    let admin = deployment.client.payer();
    let rent = deployment
        .client
        .rpc()
        .get_minimum_balance_for_rent_exemption(spl_token_2022::state::Mint::LEN)
        .await?;
    deployment
        .client
        .store_transaction()
        .signer(&mint)
        .pre_instruction(
            system_instruction::create_account(
                &admin,
                &mint.pubkey(),
                rent,
                spl_token_2022::state::Mint::LEN as u64,
                &spl_token_2022::ID,
            ),
            true,
        )
        .pre_instruction(
            spl_token_2022::instruction::initialize_mint2(
                &spl_token_2022::ID,
                &mint.pubkey(),
                &admin,
                None,
                6,
            )?,
            true,
        )
        .send()
        .await?;
    let account = deployment.client.rpc().get_account(&mint.pubkey()).await?;
    assert_eq!(account.owner, spl_token_2022::ID);
    let state = spl_token_2022::state::Mint::unpack(&account.data)?;
    assert!(state.is_initialized);
    Ok(mint.pubkey())
}

/// Exercise a real legacy SPL transfer after the Cancel lifecycle consumed nonce two.
pub(super) async fn verify_signed_withdraw(
    deployment: &Deployment,
    owner: &SecretKey,
    evm_address: [u8; 20],
    proxy: Pubkey,
    chain: [u8; 32],
    genesis: [u8; 32],
    deadline: u64,
) -> eyre::Result<()> {
    use anchor_spl::{associated_token::spl_associated_token_account, token::spl_token};
    let admin = deployment.client.payer();
    let payer = deployment
        .client
        .try_clone_with_payer(shared_signer(Keypair::new()))?;
    deployment
        .client
        .transfer(&payer.payer(), 2_000_000_000)?
        .send()
        .await?;
    let recipient = Keypair::new().pubkey();
    let (mint, source) = create_freezable_mint(deployment, &proxy).await?;
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
        nonce: 3,
        deadline,
    };
    let digest = intent.digest(&ID, &deployment.store, &genesis);
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), owner);
    let signature = signature.serialize();
    let recovery = recovery.serialize();
    let consumed_cancel_nonce = WithdrawIntent { nonce: 2, ..intent };
    let (old_signature, old_recovery) = libsecp256k1::sign(
        &Message::parse(&consumed_cancel_nonce.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &consumed_cancel_nonce,
            old_signature.serialize(),
            old_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("Cancel consumed nonce two before Withdraw");
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(6006));
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(500)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(0)
    );
    let before_legacy = snapshot(deployment, &proxy, &source, &destination).await?;
    let initial = Proxy::try_deserialize(&mut before_legacy.0.data.as_slice())?;
    assert_eq!(initial.expected_next_nonce(), intent.nonce);
    let (legacy_signature, legacy_recovery) =
        old_pair_signature(&intent, deployment.store, genesis, owner);
    let legacy = payer
        .withdraw_proxy_transaction(&intent, legacy_signature, legacy_recovery)?
        .send()
        .await
        .expect_err("legacy generation-bearing signature at the valid next nonce");
    assert_eq!(
        gmsol_sdk::Error::from(legacy).anchor_error_code(),
        Some(6011)
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &before_legacy).await?;

    let withdraw = payer.withdraw_proxy_transaction(&intent, signature, recovery)?;
    let tx_signature = withdraw.send().await?;
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
    assert_eq!(state.consumed_nonce(), 3);
    let consumed: gmsol_evm_proxy::IntentConsumed =
        super::config_policy::event(&deployment.client, &tx_signature).await?;
    assert_eq!((consumed.proxy, consumed.nonce), (proxy, 3));
    assert_eq!(consumed.kind, INTENT_KIND_WITHDRAW);
    let withdrawn: gmsol_evm_proxy::Withdrawn =
        super::config_policy::event(&deployment.client, &tx_signature).await?;
    assert_eq!(
        (
            withdrawn.proxy,
            withdrawn.nonce,
            withdrawn.mint,
            withdrawn.amount,
            withdrawn.recipient
        ),
        (proxy, 3, mint, 125, recipient)
    );

    let baseline = snapshot(deployment, &proxy, &source, &destination).await?;
    // Change only the Solana message to avoid duplicate-transaction short-circuiting.
    let exact_replay = payer
        .withdraw_proxy_transaction(&intent, signature, recovery)?
        .pre_instruction(
            system_instruction::transfer(&payer.payer(), &payer.payer(), 0),
            true,
        )
        .send()
        .await
        .expect_err("the original accepted EVM signature cannot consume the nonce twice");
    assert_eq!(
        gmsol_sdk::Error::from(exact_replay).anchor_error_code(),
        Some(6006)
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;
    let deadline = fresh_deadline(deployment).await?;
    let replay = WithdrawIntent { deadline, ..intent };
    let (replay_signature, replay_recovery) = libsecp256k1::sign(
        &Message::parse(&replay.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &replay,
            replay_signature.serialize(),
            replay_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("a consumed nonce rejects a freshly signed deadline");
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(6006));
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    let competing_cancel = CancelIntent {
        evm_address,
        chain_id: chain,
        nonce: 3,
        deadline,
    };
    let (cancel_signature, cancel_recovery) = libsecp256k1::sign(
        &Message::parse(&competing_cancel.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .cancel_proxy_transaction(
            &competing_cancel,
            cancel_signature.serialize(),
            cancel_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("Withdraw consumed nonce three before Cancel");
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(6006));
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    let missing_recipient = Keypair::new().pubkey();
    let missing_ata = WithdrawIntent {
        recipient: missing_recipient,
        nonce: 4,
        ..replay
    };
    let digest = missing_ata.digest(&ID, &deployment.store, &genesis);
    let (missing_signature, missing_recovery) = libsecp256k1::sign(&Message::parse(&digest), owner);
    let missing_address =
        spl_associated_token_account::get_associated_token_address(&missing_recipient, &mint);
    assert!(payer
        .rpc()
        .get_account_with_commitment(&missing_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());
    let err = payer
        .withdraw_proxy_transaction(
            &missing_ata,
            missing_signature.serialize(),
            missing_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("a missing destination ATA must fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::AccountNotInitialized.into())
    );
    assert!(payer
        .rpc()
        .get_account_with_commitment(&missing_address, CommitmentConfig::processed())
        .await?
        .value
        .is_none());
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    let next_intent = WithdrawIntent { nonce: 4, ..replay };
    let next_digest = next_intent.digest(&ID, &deployment.store, &genesis);
    let (next_signature, next_recovery) = libsecp256k1::sign(&Message::parse(&next_digest), owner);
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

    let wrong_owner = SecretKey::random(&mut rand_08::rngs::OsRng);
    let (wrong_signature, wrong_recovery) = libsecp256k1::sign(
        &Message::parse(&next_intent.digest(&ID, &deployment.store, &genesis)),
        &wrong_owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &next_intent,
            wrong_signature.serialize(),
            wrong_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("another EVM owner cannot withdraw");
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(6011));

    let other_chain = {
        let mut id = chain;
        id[31] = 2;
        id
    };
    let altered_domain = WithdrawIntent {
        chain_id: other_chain,
        ..next_intent
    };
    let (domain_signature, domain_recovery) = libsecp256k1::sign(
        &Message::parse(&altered_domain.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &next_intent,
            domain_signature.serialize(),
            domain_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("signature from another chain domain must fail");
    assert_eq!(gmsol_sdk::Error::from(err).anchor_error_code(), Some(6011));
    let (chain_signature, chain_recovery) = libsecp256k1::sign(
        &Message::parse(&altered_domain.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &altered_domain,
            chain_signature.serialize(),
            chain_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("disallowed chain must fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::ChainNotAllowed as u32).into())
    );

    let expired = WithdrawIntent {
        deadline: 0,
        ..next_intent
    };
    let (expired_signature, expired_recovery) = libsecp256k1::sign(
        &Message::parse(&expired.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &expired,
            expired_signature.serialize(),
            expired_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("expired intent must fail");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::InvalidDeadline as u32).into())
    );
    let wrong_decimals = WithdrawIntent {
        decimals: 7,
        ..next_intent
    };
    let (decimals_signature, decimals_recovery) = libsecp256k1::sign(
        &Message::parse(&wrong_decimals.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &wrong_decimals,
            decimals_signature.serialize(),
            decimals_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("signed decimals must match the mint");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::InvalidDecimals as u32).into())
    );

    let insufficient = WithdrawIntent {
        amount: 376,
        ..next_intent
    };
    let (insufficient_signature, insufficient_recovery) = libsecp256k1::sign(
        &Message::parse(&insufficient.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &insufficient,
            insufficient_signature.serialize(),
            insufficient_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("insufficient source balance must leave the nonce unconsumed");
    assert_token_error(err, spl_token::error::TokenError::InsufficientFunds as u32);
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    let mut wrong_token_program = gmsol_sdk::evm_proxy::withdraw_instruction(
        ID,
        payer.payer(),
        &next_intent,
        next_signature.serialize(),
        next_recovery.serialize(),
    )?;
    wrong_token_program.accounts[7].pubkey = system_program::ID;
    let err = payer
        .evm_proxy_instruction(wrong_token_program)
        .send()
        .await
        .expect_err("legacy SPL token program is required");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::InvalidProgramId.into())
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;
    let mut wrong_mint = gmsol_sdk::evm_proxy::withdraw_instruction(
        ID,
        payer.payer(),
        &next_intent,
        next_signature.serialize(),
        next_recovery.serialize(),
    )?;
    wrong_mint.accounts[3].pubkey = Keypair::new().pubkey();
    let err = payer
        .evm_proxy_instruction(wrong_mint)
        .send()
        .await
        .expect_err("mint account must match the signed canonical token accounts");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::AccountNotInitialized.into())
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;
    let mut substituted_recipient = gmsol_sdk::evm_proxy::withdraw_instruction(
        ID,
        payer.payer(),
        &next_intent,
        next_signature.serialize(),
        next_recovery.serialize(),
    )?;
    substituted_recipient.accounts[4].pubkey = Keypair::new().pubkey();
    let err = payer
        .evm_proxy_instruction(substituted_recipient)
        .send()
        .await
        .expect_err("recipient account must match the signed destination ATA");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintTokenOwner.into())
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    let token_2022_mint = create_token_2022_mint(deployment).await?;
    let token_2022_intent = WithdrawIntent {
        mint: token_2022_mint,
        ..next_intent
    };
    let (token_2022_signature, token_2022_recovery) = libsecp256k1::sign(
        &Message::parse(&token_2022_intent.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let err = payer
        .withdraw_proxy_transaction(
            &token_2022_intent,
            token_2022_signature.serialize(),
            token_2022_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("initialized Token-2022 mint must not pass the legacy mint account guard");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::AccountOwnedByWrongProgram.into())
    );
    assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

    for frozen_account in [source, destination] {
        deployment
            .client
            .store_transaction()
            .pre_instruction(
                spl_token::instruction::freeze_account(
                    &spl_token::ID,
                    &frozen_account,
                    &mint,
                    &admin,
                    &[],
                )?,
                true,
            )
            .send()
            .await?;
        let frozen = snapshot(deployment, &proxy, &source, &destination).await?;
        let err = payer
            .withdraw_proxy_transaction(
                &next_intent,
                next_signature.serialize(),
                next_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err("a frozen legacy token account must reject transfer");
        assert_token_error(err, spl_token::error::TokenError::AccountFrozen as u32);
        assert_unchanged(deployment, &proxy, &source, &destination, &frozen).await?;
        deployment
            .client
            .store_transaction()
            .pre_instruction(
                spl_token::instruction::thaw_account(
                    &spl_token::ID,
                    &frozen_account,
                    &mint,
                    &admin,
                    &[],
                )?,
                true,
            )
            .send()
            .await?;
        assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;
    }

    let zero = WithdrawIntent {
        amount: 0,
        ..next_intent
    };
    let (zero_signature, zero_recovery) = libsecp256k1::sign(
        &Message::parse(&zero.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let mut zero_ix = gmsol_sdk::evm_proxy::withdraw_instruction(
        ID,
        payer.payer(),
        &next_intent,
        next_signature.serialize(),
        next_recovery.serialize(),
    )?;
    zero_ix.data = evm_proxy::instruction::Withdraw {
        evm_address,
        chain_id: chain,
        amount: 0,
        decimals: 6,
        nonce: 4,
        deadline,
        signature: zero_signature.serialize(),
        recovery_id: zero_recovery.serialize(),
    }
    .data();
    let err = payer
        .evm_proxy_instruction(zero_ix)
        .send()
        .await
        .expect_err("production Withdraw must reject zero amount");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::ZeroAmount as u32).into())
    );

    let self_recipient = WithdrawIntent {
        recipient: proxy,
        ..next_intent
    };
    let (self_signature, self_recovery) = libsecp256k1::sign(
        &Message::parse(&self_recipient.digest(&ID, &deployment.store, &genesis)),
        owner,
    );
    let mut self_ix = gmsol_sdk::evm_proxy::withdraw_instruction(
        ID,
        payer.payer(),
        &next_intent,
        next_signature.serialize(),
        next_recovery.serialize(),
    )?;
    self_ix.accounts[4].pubkey = proxy;
    self_ix.accounts[6].pubkey = source;
    self_ix.data = evm_proxy::instruction::Withdraw {
        evm_address,
        chain_id: chain,
        amount: 125,
        decimals: 6,
        nonce: 4,
        deadline,
        signature: self_signature.serialize(),
        recovery_id: self_recovery.serialize(),
    }
    .data();
    let err = payer
        .evm_proxy_instruction(self_ix)
        .send()
        .await
        .expect_err("production Withdraw must reject proxy self-recipient");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::SelfRecipient as u32).into())
    );

    deployment
        .client
        .evm_proxy_instruction(gmsol_sdk::client::ops::evm_proxy::set_paused_instruction(
            ID, admin, true,
        ))
        .send()
        .await?;
    let err = payer
        .withdraw_proxy_transaction(
            &next_intent,
            next_signature.serialize(),
            next_recovery.serialize(),
        )?
        .send()
        .await
        .expect_err("paused Config must reject Withdraw");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some((6000 + ProxyError::Paused as u32).into())
    );
    deployment
        .client
        .evm_proxy_instruction(gmsol_sdk::client::ops::evm_proxy::set_paused_instruction(
            ID, admin, false,
        ))
        .send()
        .await?;

    assert_eq!(payer.rpc().get_account(&proxy).await?.data, account.data);
    assert_eq!(
        deployment.get_token_account_amount(&source).await?,
        Some(375)
    );
    assert_eq!(
        deployment.get_token_account_amount(&destination).await?,
        Some(125)
    );
    Ok(())
}
