use anchor_lang::{
    solana_program::keccak, AccountDeserialize, AccountSerialize, AnchorDeserialize, Discriminator,
    InstructionData, ToAccountMetas,
};
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use base64::Engine;
use gmsol_evm_proxy::{
    accounts, instruction, Config, IntentConsumed, IntentPair, Proxy, ProxyError, Withdrawn,
    CONFIG_SEED, ID, INTENT_KIND_WITHDRAW, PROXY_SEED,
};
use libsecp256k1::{Message, PublicKey, SecretKey};
use rand::rngs::OsRng;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::{Account, AccountSharedData},
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    instruction::InstructionError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    signature::Signer,
    system_program,
    transaction::{Transaction, TransactionError},
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

const NOW: i64 = 1_700_000_000;
const GENESIS: [u8; 32] = [42; 32];
const CHAIN: [u8; 32] = {
    let mut chain = [0; 32];
    chain[31] = 1;
    chain
};

fn account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: Rent::default().minimum_balance(data.len()).max(1),
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64, state: AccountState) -> Account {
    let token = TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    let mut data = vec![0; TokenAccount::LEN];
    TokenAccount::pack(token, &mut data).unwrap();
    account(spl_token::id(), data)
}

#[derive(Clone, Copy)]
struct Intent {
    mint: Pubkey,
    recipient: Pubkey,
    amount: u64,
    decimals: u8,
    generation: u64,
    nonce: u64,
    deadline: u64,
    chain_id: [u8; 32],
}

struct Fixture {
    context: ProgramTestContext,
    secret: SecretKey,
    evm_address: [u8; 20],
    proxy: Pubkey,
    config: Pubkey,
    store: Pubkey,
    source: Pubkey,
    destination: Pubkey,
    intent: Intent,
    attempts: u32,
}

async fn setup(
    source_amount: u64,
    destination_amount: u64,
    source_exists: bool,
    destination_exists: bool,
) -> Fixture {
    let secret = SecretKey::random(&mut OsRng);
    let public = PublicKey::from_secret_key(&secret).serialize();
    let evm_address: [u8; 20] = keccak::hash(&public[1..]).to_bytes()[12..]
        .try_into()
        .unwrap();
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &evm_address], &ID);
    let (config, _) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
    let store = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let source = get_associated_token_address_with_program_id(&proxy, &mint, &spl_token::id());
    let destination =
        get_associated_token_address_with_program_id(&recipient, &mint, &spl_token::id());
    let config_data = Config {
        store,
        genesis_hash: GENESIS,
        admin: Pubkey::new_unique(),
        pending_admin: None,
        paused: false,
        allowed_chain_ids: vec![CHAIN],
    };
    let mut data = Vec::new();
    config_data.try_serialize(&mut data).unwrap();
    let mut test = ProgramTest::default();
    test.prefer_bpf(true);
    test.add_program("gmsol_evm_proxy", ID, None);
    test.prefer_bpf(false);
    test.add_program(
        "spl_token",
        spl_token::id(),
        processor!(spl_token::processor::Processor::process),
    );
    test.add_program(
        "spl_associated_token_account",
        spl_associated_token_account::id(),
        processor!(spl_associated_token_account::processor::process_instruction),
    );
    test.add_program(
        "spl_token_2022",
        spl_token_2022::id(),
        processor!(spl_token_2022::processor::Processor::process),
    );
    test.add_account(config, account(ID, data));
    let mint_state = Mint {
        mint_authority: COption::None,
        supply: source_amount,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(mint_state, &mut mint_data).unwrap();
    test.add_account(mint, account(spl_token::id(), mint_data));
    test.add_account(recipient, account(system_program::id(), vec![]));
    if source_exists {
        test.add_account(
            source,
            token_account(mint, proxy, source_amount, AccountState::Initialized),
        );
    }
    if destination_exists {
        test.add_account(
            destination,
            token_account(
                mint,
                recipient,
                destination_amount,
                AccountState::Initialized,
            ),
        );
    }
    let context = test.start_with_context().await;
    let create = Instruction {
        program_id: ID,
        accounts: accounts::CreateProxy {
            payer: context.payer.pubkey(),
            proxy,
            system_program: system_program::id(),
        }
        .to_account_metas(None),
        data: instruction::CreateProxy { evm_address }.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[create],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let fixture = Fixture {
        context,
        secret,
        evm_address,
        proxy,
        config,
        store,
        source,
        destination,
        intent: Intent {
            mint,
            recipient,
            amount: 125,
            decimals: 6,
            generation: 0,
            nonce: 1,
            deadline: NOW as u64 + 120,
            chain_id: CHAIN,
        },
        attempts: 0,
    };
    let mut clock: Clock = fixture.context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = NOW;
    fixture.context.set_sysvar(&clock);
    fixture
}

fn signature(f: &Fixture, signed: Intent) -> ([u8; 64], u8) {
    let struct_hash =
        gmsol_evm_proxy_intent::withdraw_struct_hash(&gmsol_evm_proxy_intent::WithdrawFields {
            proxy: f.proxy,
            mint: signed.mint,
            amount: signed.amount,
            decimals: signed.decimals,
            recipient: signed.recipient,
            generation: signed.generation,
            nonce: signed.nonce,
            deadline: signed.deadline,
        });
    let domain =
        gmsol_evm_proxy_intent::domain_separator(&ID, &f.store, &GENESIS, &signed.chain_id);
    let digest = gmsol_evm_proxy_intent::intent_digest(&domain, &struct_hash);
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), &f.secret);
    (signature.serialize(), recovery.serialize())
}

fn withdraw_ix(f: &Fixture, input: Intent, signed: Intent) -> Instruction {
    let (signature, recovery_id) = signature(f, signed);
    Instruction {
        program_id: ID,
        accounts: accounts::Withdraw {
            submitter: f.context.payer.pubkey(),
            config: f.config,
            proxy: f.proxy,
            mint: input.mint,
            recipient: input.recipient,
            source: f.source,
            destination: f.destination,
            token_program: spl_token::id(),
        }
        .to_account_metas(None),
        data: instruction::Withdraw {
            evm_address: f.evm_address,
            chain_id: input.chain_id,
            amount: input.amount,
            decimals: input.decimals,
            generation: input.generation,
            nonce: input.nonce,
            deadline: input.deadline,
            signature,
            recovery_id,
        }
        .data(),
    }
}

async fn state(f: &mut Fixture) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let proxy = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap()
        .data;
    let source = f
        .context
        .banks_client
        .get_account(f.source)
        .await
        .unwrap()
        .map(|a| a.data);
    let destination = f
        .context
        .banks_client
        .get_account(f.destination)
        .await
        .unwrap()
        .map(|a| a.data);
    (proxy, source, destination)
}

async fn set_pair(f: &mut Fixture, pair: IntentPair) {
    let mut proxy = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    proxy.data[40..48].copy_from_slice(&pair.generation.to_le_bytes());
    proxy.data[48..56].copy_from_slice(&pair.nonce.to_le_bytes());
    f.context
        .set_account(&f.proxy, &AccountSharedData::from(proxy));
}

fn cancel_ix(f: &Fixture, generation: u64, nonce: u64) -> Instruction {
    let deadline = f.intent.deadline;
    let struct_hash =
        gmsol_evm_proxy_intent::cancel_struct_hash(&f.proxy, generation, nonce, deadline);
    let domain = gmsol_evm_proxy_intent::domain_separator(&ID, &f.store, &GENESIS, &CHAIN);
    let digest = gmsol_evm_proxy_intent::intent_digest(&domain, &struct_hash);
    let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), &f.secret);
    Instruction {
        program_id: ID,
        accounts: accounts::Cancel {
            submitter: f.context.payer.pubkey(),
            config: f.config,
            proxy: f.proxy,
        }
        .to_account_metas(None),
        data: instruction::Cancel {
            evm_address: f.evm_address,
            chain_id: CHAIN,
            generation,
            nonce,
            deadline,
            signature: signature.serialize(),
            recovery_id: recovery.serialize(),
        }
        .data(),
    }
}

async fn submit_cancel(f: &mut Fixture, ix: Instruction, success: bool) {
    let before = state(f).await;
    f.attempts += 1;
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let budget = ComputeBudgetInstruction::set_compute_unit_limit(240_000 + f.attempts);
    let tx = Transaction::new_signed_with_payer(
        &[budget, ix],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f
        .context
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    assert_eq!(result.result.is_ok(), success, "{:?}", result.result);
    let after = state(f).await;
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, before.2);
    if success {
        assert_ne!(after.0, before.0);
    } else {
        assert_eq!(after.0, before.0);
    }
}

async fn submit(f: &mut Fixture, ix: Instruction, success: bool) {
    let before = state(f).await;
    f.attempts += 1;
    let budget = ComputeBudgetInstruction::set_compute_unit_limit(240_000 + f.attempts);
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[budget, ix],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f
        .context
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    assert_eq!(result.result.is_ok(), success, "{:?}", result.result);
    let after = state(f).await;
    if !success {
        assert_eq!(after, before, "failed transaction changed protocol state");
        return;
    }
    let prior_source = TokenAccount::unpack(&before.1.unwrap()).unwrap();
    let prior_destination = TokenAccount::unpack(&before.2.unwrap()).unwrap();
    let next_source = TokenAccount::unpack(&after.1.unwrap()).unwrap();
    let next_destination = TokenAccount::unpack(&after.2.unwrap()).unwrap();
    assert_eq!(prior_source.amount - next_source.amount, f.intent.amount);
    assert_eq!(
        next_destination.amount - prior_destination.amount,
        f.intent.amount
    );
    let proxy = Proxy::try_deserialize(&mut after.0.as_slice()).unwrap();
    assert_eq!(
        proxy.consumed_pair(),
        IntentPair {
            generation: f.intent.generation,
            nonce: f.intent.nonce
        }
    );
    let logs = result.metadata.unwrap().log_messages;
    let events: Vec<Vec<u8>> = logs
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .map(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap()
        })
        .collect();
    assert_eq!(events.len(), 2);
    assert!(events[0].starts_with(IntentConsumed::DISCRIMINATOR));
    assert!(events[1].starts_with(Withdrawn::DISCRIMINATOR));
    let consumed = IntentConsumed::deserialize(&mut &events[0][8..]).unwrap();
    let withdrawn = Withdrawn::deserialize(&mut &events[1][8..]).unwrap();
    assert_eq!(consumed.kind, INTENT_KIND_WITHDRAW);
    assert_eq!(
        (consumed.proxy, consumed.generation, consumed.nonce),
        (f.proxy, f.intent.generation, f.intent.nonce)
    );
    assert_eq!(
        (
            withdrawn.proxy,
            withdrawn.generation,
            withdrawn.nonce,
            withdrawn.mint,
            withdrawn.amount,
            withdrawn.recipient
        ),
        (
            f.proxy,
            f.intent.generation,
            f.intent.nonce,
            f.intent.mint,
            f.intent.amount,
            f.intent.recipient
        )
    );
}

#[tokio::test]
async fn signed_withdraw_transfers_exact_amount_and_rejects_replay() {
    let mut f = setup(500, 10, true, true).await;
    f.intent.amount = 1;
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix.clone(), true).await;
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn exact_source_balance_transfers_without_remainder() {
    let mut f = setup(125, 0, true, true).await;
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, true).await;
    let (_, source, destination) = state(&mut f).await;
    assert_eq!(TokenAccount::unpack(&source.unwrap()).unwrap().amount, 0);
    assert_eq!(
        TokenAccount::unpack(&destination.unwrap()).unwrap().amount,
        125
    );
}

#[tokio::test]
async fn deadline_limits_are_inclusive_and_adjacent_values_fail() {
    for deadline in [NOW as u64, (NOW + 3600) as u64] {
        let mut f = setup(500, 10, true, true).await;
        f.intent.deadline = deadline;
        let ix = withdraw_ix(&f, f.intent, f.intent);
        submit(&mut f, ix, true).await;
    }
    for deadline in [(NOW - 1) as u64, (NOW + 3601) as u64] {
        let mut f = setup(500, 10, true, true).await;
        f.intent.deadline = deadline;
        let ix = withdraw_ix(&f, f.intent, f.intent);
        submit(&mut f, ix, false).await;
    }
}

#[tokio::test]
async fn token_2022_mint_is_rejected_before_transfer() {
    let mut f = setup(500, 10, true, true).await;
    let mut mint = f
        .context
        .banks_client
        .get_account(f.intent.mint)
        .await
        .unwrap()
        .unwrap();
    mint.owner = spl_token_2022::id();
    mint.data = vec![0; spl_token_2022::state::Mint::LEN];
    f.context
        .set_account(&f.intent.mint, &AccountSharedData::from(mint));
    let init = spl_token_2022::instruction::initialize_mint2(
        &spl_token_2022::id(),
        &f.intent.mint,
        &f.context.payer.pubkey(),
        None,
        6,
    )
    .unwrap();
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    f.context
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
    let mint = f
        .context
        .banks_client
        .get_account(f.intent.mint)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mint.owner, spl_token_2022::id());
    assert!(
        spl_token_2022::state::Mint::unpack(&mint.data)
            .unwrap()
            .is_initialized
    );
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn recipient_ata_created_earlier_in_transaction_is_accepted() {
    let mut f = setup(500, 0, true, false).await;
    let create_ata = spl_associated_token_account::instruction::create_associated_token_account(
        &f.context.payer.pubkey(),
        &f.intent.recipient,
        &f.intent.mint,
        &spl_token::id(),
    );
    let withdraw = withdraw_ix(&f, f.intent, f.intent);
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[create_ata, withdraw],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f
        .context
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    assert!(result.result.is_ok(), "{:?}", result.result);
    let (_, source, destination) = state(&mut f).await;
    assert_eq!(TokenAccount::unpack(&source.unwrap()).unwrap().amount, 375);
    assert_eq!(
        TokenAccount::unpack(&destination.unwrap()).unwrap().amount,
        125
    );
    let proxy = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    let proxy = Proxy::try_deserialize(&mut proxy.data.as_slice()).unwrap();
    assert_eq!(
        proxy.consumed_pair(),
        IntentPair {
            generation: 0,
            nonce: 1
        }
    );
}

#[tokio::test]
async fn invalid_effects_and_accounts_leave_balances_and_pair_unchanged() {
    let mut f = setup(500, 10, true, true).await;
    for changed in [
        Intent {
            amount: 0,
            ..f.intent
        },
        Intent {
            amount: 501,
            ..f.intent
        },
        Intent {
            decimals: 7,
            ..f.intent
        },
        Intent {
            generation: 1,
            ..f.intent
        },
        Intent {
            nonce: 2,
            ..f.intent
        },
        Intent {
            deadline: (NOW - 1) as u64,
            ..f.intent
        },
        Intent {
            deadline: (NOW + 3601) as u64,
            ..f.intent
        },
        Intent {
            chain_id: [0; 32],
            ..f.intent
        },
    ] {
        let ix = withdraw_ix(&f, changed, changed);
        submit(&mut f, ix, false).await;
    }
    let changed = Intent {
        recipient: Pubkey::new_unique(),
        ..f.intent
    };
    let ix = withdraw_ix(&f, changed, f.intent);
    submit(&mut f, ix, false).await;
    let changed = Intent {
        amount: 126,
        ..f.intent
    };
    let ix = withdraw_ix(&f, changed, f.intent);
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn self_recipient_hits_explicit_guard_with_canonical_self_ata() {
    let mut f = setup(500, 10, true, true).await;
    f.intent.recipient = f.proxy;
    f.destination = f.source;
    let before = state(&mut f).await;
    let ix = withdraw_ix(&f, f.intent, f.intent);
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f
        .context
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    assert_eq!(
        result.result,
        Err(TransactionError::InstructionError(
            0,
            InstructionError::Custom(6000 + ProxyError::SelfRecipient as u32)
        ))
    );
    assert_eq!(state(&mut f).await, before);
}

#[tokio::test]
async fn missing_and_frozen_token_accounts_fail_atomically() {
    for (source_exists, destination_exists) in [(false, true), (true, false)] {
        let mut f = setup(500, 10, source_exists, destination_exists).await;
        let ix = withdraw_ix(&f, f.intent, f.intent);
        submit(&mut f, ix, false).await;
    }
    for freeze_source in [true, false] {
        let mut f = setup(500, 10, true, true).await;
        let key = if freeze_source {
            f.source
        } else {
            f.destination
        };
        let mut token = f
            .context
            .banks_client
            .get_account(key)
            .await
            .unwrap()
            .unwrap();
        let mut state = TokenAccount::unpack(&token.data).unwrap();
        state.state = AccountState::Frozen;
        TokenAccount::pack(state, &mut token.data).unwrap();
        f.context.set_account(&key, &AccountSharedData::from(token));
        let ix = withdraw_ix(&f, f.intent, f.intent);
        submit(&mut f, ix, false).await;
    }
}

#[tokio::test]
async fn canonical_accounts_and_proxy_authority_reject_substitution() {
    let mut f = setup(500, 10, true, true).await;
    let mut wrong_source = withdraw_ix(&f, f.intent, f.intent);
    let source_key = Pubkey::new_unique();
    f.context.set_account(
        &source_key,
        &AccountSharedData::from(token_account(
            f.intent.mint,
            f.proxy,
            500,
            AccountState::Initialized,
        )),
    );
    wrong_source.accounts[5].pubkey = source_key;
    submit(&mut f, wrong_source, false).await;

    let mut wrong_destination = withdraw_ix(&f, f.intent, f.intent);
    let destination_key = Pubkey::new_unique();
    f.context.set_account(
        &destination_key,
        &AccountSharedData::from(token_account(
            f.intent.mint,
            f.intent.recipient,
            10,
            AccountState::Initialized,
        )),
    );
    wrong_destination.accounts[6].pubkey = destination_key;
    submit(&mut f, wrong_destination, false).await;

    let mut wrong_proxy = withdraw_ix(&f, f.intent, f.intent);
    wrong_proxy.data[8] ^= 1;
    submit(&mut f, wrong_proxy, false).await;

    let mut token = f
        .context
        .banks_client
        .get_account(f.source)
        .await
        .unwrap()
        .unwrap();
    let mut source = TokenAccount::unpack(&token.data).unwrap();
    source.owner = f.context.payer.pubkey();
    TokenAccount::pack(source, &mut token.data).unwrap();
    f.context
        .set_account(&f.source, &AccountSharedData::from(token));
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn delegate_cannot_replace_signed_proxy_authority() {
    let mut f = setup(500, 10, true, true).await;
    let mut token = f
        .context
        .banks_client
        .get_account(f.source)
        .await
        .unwrap()
        .unwrap();
    let mut source = TokenAccount::unpack(&token.data).unwrap();
    source.delegate = COption::Some(f.context.payer.pubkey());
    source.delegated_amount = 500;
    TokenAccount::pack(source, &mut token.data).unwrap();
    f.context
        .set_account(&f.source, &AccountSharedData::from(token));
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, true).await;
    let source = f
        .context
        .banks_client
        .get_account(f.source)
        .await
        .unwrap()
        .unwrap();
    let source = TokenAccount::unpack(&source.data).unwrap();
    assert_eq!(source.delegate, COption::Some(f.context.payer.pubkey()));
    assert_eq!(source.delegated_amount, 500);
}

#[tokio::test]
async fn submitter_cannot_transfer_delegate_or_change_proxy_token_authority() {
    let mut f = setup(500, 10, true, true).await;
    let payer = f.context.payer.pubkey();
    let transfer = spl_token::instruction::transfer_checked(
        &spl_token::id(),
        &f.source,
        &f.intent.mint,
        &f.destination,
        &payer,
        &[],
        125,
        6,
    )
    .unwrap();
    let approve = spl_token::instruction::approve_checked(
        &spl_token::id(),
        &f.source,
        &f.intent.mint,
        &payer,
        &payer,
        &[],
        125,
        6,
    )
    .unwrap();
    let change_owner = spl_token::instruction::set_authority(
        &spl_token::id(),
        &f.source,
        Some(&payer),
        spl_token::instruction::AuthorityType::AccountOwner,
        &payer,
        &[],
    )
    .unwrap();
    for ix in [transfer, approve, change_owner] {
        submit(&mut f, ix, false).await;
    }

    let mut empty = setup(0, 10, true, true).await;
    let payer = empty.context.payer.pubkey();
    let close =
        spl_token::instruction::close_account(&spl_token::id(), &empty.source, &payer, &payer, &[])
            .unwrap();
    submit(&mut empty, close, false).await;
}

#[tokio::test]
async fn paused_config_and_wrong_signature_fail_without_consumption() {
    let mut f = setup(500, 10, true, true).await;
    let mut wrong_signature = withdraw_ix(&f, f.intent, f.intent);
    let signature_byte = wrong_signature.data.len() - 2;
    wrong_signature.data[signature_byte] ^= 1;
    submit(&mut f, wrong_signature, false).await;

    let mut config_account = f
        .context
        .banks_client
        .get_account(f.config)
        .await
        .unwrap()
        .unwrap();
    let mut config = Config::try_deserialize(&mut config_account.data.as_slice()).unwrap();
    config.paused = true;
    config_account.data.clear();
    config.try_serialize(&mut config_account.data).unwrap();
    f.context
        .set_account(&f.config, &AccountSharedData::from(config_account));
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn destination_overflow_and_terminal_pair_fail_atomically() {
    let mut f = setup(500, u64::MAX, true, true).await;
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, false).await;
    let mut f = setup(500, 10, true, true).await;
    set_pair(
        &mut f,
        IntentPair {
            generation: u64::MAX,
            nonce: u64::MAX,
        },
    )
    .await;
    let ix = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, ix, false).await;
}

#[tokio::test]
async fn cancel_and_withdraw_compete_for_one_pair_in_both_orders() {
    let mut f = setup(500, 10, true, true).await;
    let withdraw = withdraw_ix(&f, f.intent, f.intent);
    let cancel = cancel_ix(&f, 0, 1);
    submit_cancel(&mut f, cancel, true).await;
    submit(&mut f, withdraw, false).await;

    let mut f = setup(500, 10, true, true).await;
    let withdraw = withdraw_ix(&f, f.intent, f.intent);
    let cancel = cancel_ix(&f, 0, 1);
    submit(&mut f, withdraw, true).await;
    submit_cancel(&mut f, cancel, false).await;
}

#[tokio::test]
async fn signed_generation_controls_future_presigning_and_rollover() {
    let mut f = setup(500, 10, true, true).await;
    set_pair(
        &mut f,
        IntentPair {
            generation: 7,
            nonce: u64::MAX - 1,
        },
    )
    .await;
    let future = Intent {
        generation: 8,
        nonce: 0,
        ..f.intent
    };
    let premature = withdraw_ix(&f, future, future);
    submit(&mut f, premature, false).await;
    f.intent = Intent {
        generation: 7,
        nonce: u64::MAX,
        ..f.intent
    };
    let maximum = withdraw_ix(&f, f.intent, f.intent);
    submit(&mut f, maximum, true).await;
    let stale = Intent {
        generation: 7,
        nonce: 0,
        ..f.intent
    };
    let stale_ix = withdraw_ix(&f, stale, stale);
    submit(&mut f, stale_ix, false).await;
    f.intent = future;
    let eligible = withdraw_ix(&f, future, future);
    submit(&mut f, eligible, true).await;
}

#[tokio::test]
async fn later_instruction_failure_rolls_back_transfer_pair_and_events() {
    let mut f = setup(500, 10, true, true).await;
    let before = state(&mut f).await;
    let withdraw = withdraw_ix(&f, f.intent, f.intent);
    let invalid = Instruction {
        program_id: ID,
        accounts: vec![],
        data: vec![0xff; 8],
    };
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[withdraw.clone(), invalid],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f
        .context
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    assert!(matches!(
        result.result,
        Err(TransactionError::InstructionError(1, _))
    ));
    let logs = result.metadata.unwrap().log_messages;
    let events: Vec<_> = logs
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .collect();
    assert_eq!(events.len(), 2, "withdraw must reach both event writes");
    assert_eq!(state(&mut f).await, before);
    submit(&mut f, withdraw, true).await;
}

#[test]
fn canonical_withdraw_hashes_match_independent_viem_vectors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/withdraw-vectors.json")).unwrap();
    assert_eq!(fixture["generator"], "viem@2.57.7");
    assert_eq!(fixture["type"], gmsol_evm_proxy_intent::WITHDRAW_TYPE);
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 12);
    let decode = |name: &str, vector: &serde_json::Value| -> [u8; 32] {
        let hex = vector[name].as_str().unwrap().strip_prefix("0x").unwrap();
        std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
    };
    for v in vectors {
        let key = |name| Pubkey::new_from_array(decode(name, v));
        let mint = key("mint");
        let recipient = key("recipient");
        assert_eq!(mint.to_string(), v["mintBase58"]);
        assert_eq!(recipient.to_string(), v["recipientBase58"]);
        let number = |name| v[name].as_str().unwrap().parse::<u64>().unwrap();
        let salt = gmsol_evm_proxy_intent::domain_salt(
            &key("program"),
            &key("store"),
            &decode("genesis", v),
        );
        assert_eq!(salt, decode("salt", v));
        let domain = gmsol_evm_proxy_intent::domain_separator(
            &key("program"),
            &key("store"),
            &decode("genesis", v),
            &decode("chainId", v),
        );
        assert_eq!(domain, decode("domainSeparator", v));
        let struct_hash =
            gmsol_evm_proxy_intent::withdraw_struct_hash(&gmsol_evm_proxy_intent::WithdrawFields {
                proxy: key("proxy"),
                mint,
                amount: number("amount"),
                decimals: v["decimals"].as_u64().unwrap() as u8,
                recipient,
                generation: number("generation"),
                nonce: number("nonce"),
                deadline: number("deadline"),
            });
        assert_eq!(struct_hash, decode("structHash", v));
        assert_eq!(
            gmsol_evm_proxy_intent::intent_digest(&domain, &struct_hash),
            decode("digest", v)
        );
    }
}
