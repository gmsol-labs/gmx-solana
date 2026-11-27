use super::setup::{current_deployment, Deployment, EVM_PROXY_CONFIG_LOCK};
use evm_proxy::{Proxy, ProxyError, ID, INTENT_KIND_WITHDRAW};
use futures_util::FutureExt;
use gmsol_evm_proxy as evm_proxy;
use gmsol_programs::anchor_lang::{error::ErrorCode, AccountDeserialize, InstructionData};
use gmsol_sdk::client::ops::{EvmProxyOps, SystemProgramOps};
use gmsol_sdk::evm_proxy::CancelIntent;
use gmsol_sdk::evm_proxy::{ProxyCPIEvent, WithdrawIntent};
use gmsol_solana_utils::signer::shared_signer;
use libsecp256k1::{Message, SecretKey};
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    instruction::InstructionError,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_program,
    transaction::TransactionError,
};
use std::panic::AssertUnwindSafe;

pub(super) const WITHDRAW_SOURCE_AMOUNT: u64 = 500;
pub(super) const WITHDRAW_AMOUNT: u64 = 125;
pub(super) const WITHDRAW_MINT_DECIMALS: u8 = 6;

const INTENT_VALIDITY_SECONDS: u64 = 300;

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
        .checked_add(INTENT_VALIDITY_SECONDS)
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

async fn create_legacy_mint(
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
                None,
                WITHDRAW_MINT_DECIMALS,
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
                WITHDRAW_MINT_DECIMALS,
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

pub(super) async fn funded_withdraw_accounts(
    deployment: &Deployment,
    proxy: &Pubkey,
    recipient: Pubkey,
) -> eyre::Result<(Pubkey, Pubkey, Pubkey)> {
    use anchor_spl::{associated_token::spl_associated_token_account, token::spl_token};
    let admin = deployment.client.payer();
    let (mint, source) = create_legacy_mint(deployment, proxy).await?;
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
                WITHDRAW_SOURCE_AMOUNT,
                WITHDRAW_MINT_DECIMALS,
            )?,
            true,
        )
        .send()
        .await?;
    Ok((mint, source, destination))
}

#[tokio::test]
async fn withdraw_enforces_signed_fields_legacy_atas_and_atomic_nonce() -> eyre::Result<()> {
    const FUNDED_PAYER_LAMPORTS: u64 = 2_000_000_000;
    let deployment = current_deployment().await?;
    let _accounts = deployment.use_accounts().await?;
    let _policy = EVM_PROXY_CONFIG_LOCK.lock().await;
    let baseline_policy = super::config_policy::assert_baseline(&deployment).await?;
    let deployment = &*deployment;
    let outcome = AssertUnwindSafe(async {
        let genesis = baseline_policy.genesis_hash;
        let owner_key = loop {
            if let Ok(key) = SecretKey::parse(&rand::random()) {
                break key;
            }
        };
        let owner = &owner_key;
        let public = libsecp256k1::PublicKey::from_secret_key(owner).serialize();
        let hash = solana_sdk::keccak::hash(&public[1..]);
        let evm_address = hash.to_bytes()[12..]
            .try_into()
            .expect("20-byte EVM address");
        let proxy = gmsol_sdk::pda::find_proxy_address(&evm_address, &ID).0;
        let client = &deployment.client;
        client
            .store_transaction()
            .program(ID)
            .anchor_args(evm_proxy::instruction::CreateProxyIdempotent { evm_address })
            .anchor_accounts(evm_proxy::accounts::CreateProxyIdempotent {
                payer: client.payer(),
                proxy,
                system_program: system_program::ID,
            })
            .send()
            .await?;
        let mut chain = [0; 32];
        chain[chain.len() - 1] = 1;
        client
            .set_proxy_chain_allowed(ID, chain, true)
            .send()
            .await?;
        client.set_proxy_paused(ID, false).send().await?;
        for nonce in [0, 1] {
            let cancel = CancelIntent {
                evm_address,
                chain_id: chain,
                nonce,
                deadline: fresh_deadline(deployment).await?,
            };
            let (signature, recovery) = libsecp256k1::sign(
                &Message::parse(&cancel.digest(&ID, &deployment.store, &genesis)),
                owner,
            );
            client
                .cancel_proxy_intent(ID, &cancel, signature.serialize(), recovery.serialize())?
                .send()
                .await?;
        }
        let previous = client.rpc().get_account(&proxy).await?;
        let previous = Proxy::try_deserialize(&mut previous.data.as_slice())?;
        let initial_nonce = previous.next_nonce;
        let consumed_cancel_nonce_value = previous
            .next_nonce
            .checked_sub(1)
            .expect("preceding Cancel consumed a nonce");
        let mut withdrawal_chain = chain;
        withdrawal_chain[31] = 2;
        client
            .set_proxy_chain_allowed(ID, withdrawal_chain, true)
            .send()
            .await?;
        let deadline = fresh_deadline(deployment).await?;
        use anchor_spl::{associated_token::spl_associated_token_account, token::spl_token};
        let admin = deployment.client.payer();
        let payer = deployment
            .client
            .try_clone_with_payer(shared_signer(Keypair::new()))?;
        deployment
            .client
            .transfer(&payer.payer(), FUNDED_PAYER_LAMPORTS)?
            .send()
            .await?;
        let recipient = Keypair::new().pubkey();
        let (mint, source, destination) =
            funded_withdraw_accounts(deployment, &proxy, recipient).await?;
        assert_eq!(
            deployment.get_token_account_amount(&source).await?,
            Some(WITHDRAW_SOURCE_AMOUNT)
        );
        assert_eq!(
            deployment.get_token_account_amount(&destination).await?,
            Some(0)
        );

        let intent = WithdrawIntent {
            evm_address,
            chain_id: withdrawal_chain,
            mint,
            amount: WITHDRAW_AMOUNT,
            decimals: WITHDRAW_MINT_DECIMALS,
            recipient,
            nonce: initial_nonce,
            deadline,
        };
        let digest = intent.digest(&ID, &deployment.store, &genesis);
        let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), owner);
        let signature = signature.serialize();
        let recovery = recovery.serialize();
        let consumed_cancel_nonce = WithdrawIntent {
            nonce: consumed_cancel_nonce_value,
            ..intent
        };
        let (old_signature, old_recovery) = libsecp256k1::sign(
            &Message::parse(&consumed_cancel_nonce.digest(&ID, &deployment.store, &genesis)),
            owner,
        );
        let err = payer
            .withdraw_proxy_tokens(
                ID,
                &consumed_cancel_nonce,
                old_signature.serialize(),
                old_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err(
                "Cancel on the first chain consumed this nonce before Withdraw on the second chain",
            );
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ProxyError::InvalidIntentNonce.into())
        );
        assert_eq!(
            deployment.get_token_account_amount(&source).await?,
            Some(WITHDRAW_SOURCE_AMOUNT)
        );
        assert_eq!(
            deployment.get_token_account_amount(&destination).await?,
            Some(0)
        );
        let withdraw = payer.withdraw_proxy_tokens(ID, &intent, signature, recovery)?;
        let tx_signature = withdraw.send().await?;
        assert_eq!(
            deployment.get_token_account_amount(&source).await?,
            Some(WITHDRAW_SOURCE_AMOUNT - WITHDRAW_AMOUNT)
        );
        assert_eq!(
            deployment.get_token_account_amount(&destination).await?,
            Some(WITHDRAW_AMOUNT)
        );
        let account = payer.rpc().get_account(&proxy).await?;
        let state = Proxy::try_deserialize(&mut account.data.as_slice())?;
        assert_eq!(state.next_nonce, intent.nonce + 1);
        let next_nonce = state.next_nonce;
        let events = super::config_policy::events(&deployment.client, &tx_signature).await?;
        let [ProxyCPIEvent::IntentConsumed(consumed), ProxyCPIEvent::Withdrawn(withdrawn)] =
            events.as_slice()
        else {
            eyre::bail!("expected ordered intent and withdrawal CPI events");
        };
        assert_eq!((consumed.proxy, consumed.nonce), (proxy, intent.nonce));
        assert_eq!(consumed.kind, INTENT_KIND_WITHDRAW);
        assert_eq!(
            (
                withdrawn.proxy,
                withdrawn.nonce,
                withdrawn.mint,
                withdrawn.amount,
                withdrawn.recipient
            ),
            (proxy, intent.nonce, mint, WITHDRAW_AMOUNT, recipient)
        );

        let baseline = snapshot(deployment, &proxy, &source, &destination).await?;
        // Change only the Solana message to avoid duplicate-transaction short-circuiting.
        let exact_replay = payer
            .withdraw_proxy_tokens(ID, &intent, signature, recovery)?
            .pre_instruction(
                system_instruction::transfer(&payer.payer(), &payer.payer(), 0),
                true,
            )
            .send()
            .await
            .expect_err("the original accepted EVM signature cannot consume the nonce twice");
        assert_eq!(
            gmsol_sdk::Error::from(exact_replay).anchor_error_code(),
            Some(ProxyError::InvalidIntentNonce.into())
        );
        assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;
        let deadline = fresh_deadline(deployment).await?;
        let replay = WithdrawIntent { deadline, ..intent };
        let (replay_signature, replay_recovery) = libsecp256k1::sign(
            &Message::parse(&replay.digest(&ID, &deployment.store, &genesis)),
            owner,
        );
        let err = payer
            .withdraw_proxy_tokens(
                ID,
                &replay,
                replay_signature.serialize(),
                replay_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err("a consumed nonce rejects a freshly signed deadline");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ProxyError::InvalidIntentNonce.into())
        );
        assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

        let competing_cancel = CancelIntent {
            evm_address,
            chain_id: chain,
            nonce: initial_nonce,
            deadline,
        };
        let (cancel_signature, cancel_recovery) = libsecp256k1::sign(
            &Message::parse(&competing_cancel.digest(&ID, &deployment.store, &genesis)),
            owner,
        );
        let err = payer
            .cancel_proxy_intent(
                ID,
                &competing_cancel,
                cancel_signature.serialize(),
                cancel_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err(
                "Withdraw on the second chain consumed this nonce before Cancel on the first chain",
            );
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ProxyError::InvalidIntentNonce.into())
        );
        assert_unchanged(deployment, &proxy, &source, &destination, &baseline).await?;

        let missing_recipient = Keypair::new().pubkey();
        let missing_ata = WithdrawIntent {
            recipient: missing_recipient,
            nonce: next_nonce,
            ..replay
        };
        let digest = missing_ata.digest(&ID, &deployment.store, &genesis);
        let (missing_signature, missing_recovery) =
            libsecp256k1::sign(&Message::parse(&digest), owner);
        let missing_address =
            spl_associated_token_account::get_associated_token_address(&missing_recipient, &mint);
        assert!(payer
            .rpc()
            .get_account_with_commitment(&missing_address, CommitmentConfig::processed())
            .await?
            .value
            .is_none());
        let err = payer
            .withdraw_proxy_tokens(
                ID,
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

        let next_intent = WithdrawIntent {
            nonce: next_nonce,
            ..replay
        };
        let next_digest = next_intent.digest(&ID, &deployment.store, &genesis);
        let (next_signature, next_recovery) =
            libsecp256k1::sign(&Message::parse(&next_digest), owner);

        // Both substitutions satisfy the account constraints, so only signature binding rejects them.
        let alternate_recipient = Pubkey::new_unique();
        let alternate_destination =
            spl_associated_token_account::get_associated_token_address(&alternate_recipient, &mint);
        client
        .store_transaction()
        .pre_instruction(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &admin,
                &alternate_recipient,
                &mint,
                &spl_token::ID,
            ),
            true,
        )
        .send()
        .await?;
        let (alternate_mint, alternate_source, alternate_mint_destination) =
            funded_withdraw_accounts(deployment, &proxy, recipient).await?;
        assert_eq!(
            deployment
                .get_token_account_amount(&alternate_source)
                .await?,
            Some(WITHDRAW_SOURCE_AMOUNT)
        );
        assert_eq!(
            deployment
                .get_token_account_amount(&alternate_mint_destination)
                .await?,
            Some(0)
        );
        let substitutions = [
            (
                "coherent recipient and existing ATA",
                WithdrawIntent {
                    recipient: alternate_recipient,
                    ..next_intent
                },
            ),
            (
                "coherent initialized mint and funded ATAs",
                WithdrawIntent {
                    mint: alternate_mint,
                    ..next_intent
                },
            ),
        ];
        let protected = [
            proxy,
            source,
            destination,
            alternate_destination,
            alternate_source,
            alternate_mint_destination,
        ];
        let before = client
            .rpc()
            .get_multiple_accounts_with_commitment(&protected, CommitmentConfig::confirmed())
            .await?
            .value;
        for (reason, changed) in substitutions {
            let error = payer
                .withdraw_proxy_tokens(
                    ID,
                    &changed,
                    next_signature.serialize(),
                    next_recovery.serialize(),
                )?
                .send()
                .await
                .expect_err(reason);
            assert_eq!(
                gmsol_sdk::Error::from(error).anchor_error_code(),
                Some(ProxyError::InvalidSignature.into()),
                "{reason}"
            );
            assert_eq!(
                client
                    .rpc()
                    .get_multiple_accounts_with_commitment(
                        &protected,
                        CommitmentConfig::confirmed()
                    )
                    .await?
                    .value,
                before,
                "{reason}"
            );
        }

        let changed_amount = WithdrawIntent {
            amount: WITHDRAW_AMOUNT + 1,
            ..next_intent
        };
        let err = payer
            .withdraw_proxy_tokens(
                ID,
                &changed_amount,
                next_signature.serialize(),
                next_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err("a changed amount invalidates the signed intent");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ProxyError::InvalidSignature.into())
        );
        assert_eq!(
            deployment.get_token_account_amount(&source).await?,
            Some(WITHDRAW_SOURCE_AMOUNT - WITHDRAW_AMOUNT)
        );
        assert_eq!(
            deployment.get_token_account_amount(&destination).await?,
            Some(WITHDRAW_AMOUNT)
        );
        assert_eq!(payer.rpc().get_account(&proxy).await?.data, account.data);

        let wrong_decimals = WithdrawIntent {
            decimals: WITHDRAW_MINT_DECIMALS + 1,
            ..next_intent
        };
        let (decimals_signature, decimals_recovery) = libsecp256k1::sign(
            &Message::parse(&wrong_decimals.digest(&ID, &deployment.store, &genesis)),
            owner,
        );
        let err = payer
            .withdraw_proxy_tokens(
                ID,
                &wrong_decimals,
                decimals_signature.serialize(),
                decimals_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err("signed decimals must match the mint");
        assert_eq!(
            gmsol_sdk::Error::from(err).anchor_error_code(),
            Some(ProxyError::InvalidDecimals.into())
        );

        let insufficient = WithdrawIntent {
            amount: WITHDRAW_SOURCE_AMOUNT - WITHDRAW_AMOUNT + 1,
            ..next_intent
        };
        let (insufficient_signature, insufficient_recovery) = libsecp256k1::sign(
            &Message::parse(&insufficient.digest(&ID, &deployment.store, &genesis)),
            owner,
        );
        let err = payer
            .withdraw_proxy_tokens(
                ID,
                &insufficient,
                insufficient_signature.serialize(),
                insufficient_recovery.serialize(),
            )?
            .send()
            .await
            .expect_err("insufficient source balance must leave the nonce unconsumed");
        assert_token_error(err, spl_token::error::TokenError::InsufficientFunds as u32);
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
            .withdraw_proxy_tokens(
                ID,
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
            chain_id: withdrawal_chain,
            amount: 0,
            decimals: WITHDRAW_MINT_DECIMALS,
            nonce: next_nonce,
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
            Some(ProxyError::ZeroAmount.into())
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
            chain_id: withdrawal_chain,
            amount: WITHDRAW_AMOUNT,
            decimals: WITHDRAW_MINT_DECIMALS,
            nonce: next_nonce,
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
            Some(ProxyError::SelfRecipient.into())
        );

        assert_eq!(payer.rpc().get_account(&proxy).await?.data, account.data);
        assert_eq!(
            deployment.get_token_account_amount(&source).await?,
            Some(WITHDRAW_SOURCE_AMOUNT - WITHDRAW_AMOUNT)
        );
        assert_eq!(
            deployment.get_token_account_amount(&destination).await?,
            Some(WITHDRAW_AMOUNT)
        );
        Ok(())
    })
    .catch_unwind()
    .await;
    let restored = super::config_policy::restore_baseline(deployment, &[]).await;
    super::config_policy::finish_policy_test(outcome, restored)
}
