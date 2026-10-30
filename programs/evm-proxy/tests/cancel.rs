use anchor_lang::{
    AccountDeserialize, AccountSerialize, AnchorDeserialize, Discriminator, InstructionData,
    ToAccountMetas,
};
use base64::Engine;
use gmsol_evm_proxy::{
    accounts, instruction, Config, IntentConsumed, IntentPair, Proxy, CONFIG_SEED, ID,
    INTENT_KIND_CANCEL, PROXY_SEED,
};
use libsecp256k1::{Message, SecretKey};
use rand::rngs::OsRng;
use serde_json::Value;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::{Account, AccountSharedData},
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};
use std::collections::HashSet;

const INVALID_PROXY: u32 = 6000;
const INVALID_PAIR: u32 = 6006;
const GENERATION_OVERFLOW: u32 = 6007;
const PAUSED: u32 = 6008;
const CHAIN_NOT_ALLOWED: u32 = 6009;
const INVALID_DEADLINE: u32 = 6010;
const INVALID_SIGNATURE: u32 = 6011;

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    let hex = hex.strip_prefix("0x").unwrap();
    assert_eq!(hex.len(), N * 2);
    std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
}

fn fixture() -> Value {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/cancel-pair-vectors.json")).unwrap();
    assert_eq!(fixture["generator"], "viem@2.57.7");
    assert_eq!(fixture["programAddress"], ID.to_string());
    assert_eq!(fixture["vectors"].as_array().unwrap().len(), 22);
    fixture
}

fn vector(name: &str) -> Value {
    fixture()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
        .clone()
}

fn field<const N: usize>(vector: &Value, name: &str) -> [u8; N] {
    bytes(vector[name].as_str().unwrap())
}

fn number(vector: &Value, name: &str) -> u64 {
    vector[name].as_str().unwrap().parse().unwrap()
}

fn pair(generation: u64, nonce: u64) -> IntentPair {
    IntentPair { generation, nonce }
}

fn config_account(config: &Config) -> Account {
    let mut data = Vec::new();
    config.try_serialize(&mut data).unwrap();
    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: ID,
        executable: false,
        rent_epoch: 0,
    }
}

struct Fixture {
    context: ProgramTestContext,
    config: Pubkey,
    proxy: Pubkey,
    evm_address: [u8; 20],
    submitter: Keypair,
    submissions: u32,
}

async fn setup() -> Fixture {
    let source = fixture();
    let base = vector("first");
    let evm_address = bytes(source["evmAddress"].as_str().unwrap());
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &evm_address], &ID);
    assert_eq!(
        proxy.to_bytes(),
        bytes::<32>(source["canonicalProxyAccount"].as_str().unwrap())
    );
    let (config, _) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
    let value = Config {
        store: Pubkey::new_from_array(field(&base, "store")),
        genesis_hash: field(&base, "genesis"),
        admin: Pubkey::new_unique(),
        pending_admin: None,
        paused: false,
        allowed_chain_ids: vec![
            field(&base, "chainId"),
            field(&vector("other-chain"), "chainId"),
            field(&vector("full-width-chain"), "chainId"),
        ],
    };
    let submitter = Keypair::new();
    let mut program = ProgramTest::default();
    program.prefer_bpf(true);
    program.add_program("gmsol_evm_proxy", ID, None);
    program.add_account(config, config_account(&value));
    program.add_account(
        submitter.pubkey(),
        Account {
            lamports: 1_000_000,
            data: Vec::new(),
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let context = program.start_with_context().await;
    let create = Instruction {
        program_id: ID,
        accounts: accounts::CreateProxy {
            payer: context.payer.pubkey(),
            proxy,
            system_program: system_program::ID,
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
    let now: i64 = source["now"].as_str().unwrap().parse().unwrap();
    let mut clock: Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = now;
    context.set_sysvar(&clock);
    Fixture {
        context,
        config,
        proxy,
        evm_address,
        submitter,
        submissions: 0,
    }
}

fn cancel(f: &Fixture, input: &Value, signed: &Value) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::Cancel {
            submitter: f.submitter.pubkey(),
            config: f.config,
            proxy: f.proxy,
        }
        .to_account_metas(None),
        data: instruction::Cancel {
            evm_address: f.evm_address,
            chain_id: field(input, "chainId"),
            generation: number(input, "generation"),
            nonce: number(input, "nonce"),
            deadline: number(input, "deadline"),
            signature: field(signed, "signature"),
            recovery_id: signed["recoveryId"].as_u64().unwrap().try_into().unwrap(),
        }
        .data(),
    }
}

fn intent_events(logs: &[String]) -> Vec<IntentConsumed> {
    logs.iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .map(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap()
        })
        .map(|data| {
            assert!(data.starts_with(IntentConsumed::DISCRIMINATOR));
            IntentConsumed::deserialize(&mut &data[8..]).unwrap()
        })
        .collect()
}

async fn submit(f: &mut Fixture, ix: Instruction, expected: Result<IntentPair, u32>) {
    let mut seen = HashSet::new();
    let mut before = Vec::new();
    for key in [
        f.config,
        f.proxy,
        ix.accounts[1].pubkey,
        ix.accounts[2].pubkey,
        f.submitter.pubkey(),
    ] {
        if seen.insert(key) {
            before.push((key, f.context.banks_client.get_account(key).await.unwrap()));
        }
    }
    f.submissions += 1;
    let budget = ComputeBudgetInstruction::set_compute_unit_limit(200_000 + f.submissions);
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[budget, ix],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer, &f.submitter],
        blockhash,
    );
    let outcome = f
        .context
        .banks_client
        .process_transaction_with_metadata(transaction)
        .await
        .unwrap();
    let logs = outcome.metadata.unwrap().log_messages;
    match expected {
        Ok(pair) => {
            assert_eq!(outcome.result, Ok(()));
            let events = intent_events(&logs);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].proxy, f.proxy);
            assert_eq!(events[0].generation, pair.generation);
            assert_eq!(events[0].nonce, pair.nonce);
            assert_eq!(events[0].kind, INTENT_KIND_CANCEL);
        }
        Err(code) => {
            assert_eq!(
                outcome.result,
                Err(TransactionError::InstructionError(
                    1,
                    InstructionError::Custom(code)
                ))
            );
            assert!(intent_events(&logs).is_empty());
        }
    }
    for (key, previous) in before {
        let actual = f.context.banks_client.get_account(key).await.unwrap();
        if key == f.proxy {
            if let Ok(pair) = expected {
                let mut changed = previous.unwrap();
                changed.data[40..48].copy_from_slice(&pair.generation.to_le_bytes());
                changed.data[48..56].copy_from_slice(&pair.nonce.to_le_bytes());
                assert_eq!(actual, Some(changed));
                continue;
            }
        }
        assert_eq!(actual, previous);
    }
}

async fn submit_signed(
    f: &mut Fixture,
    input: &Value,
    signed: &Value,
    expected: Result<IntentPair, u32>,
) {
    let ix = cancel(f, input, signed);
    submit(f, ix, expected).await;
}

async fn set_pair(f: &mut Fixture, pair: IntentPair) {
    let mut account = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    account.data[40..48].copy_from_slice(&pair.generation.to_le_bytes());
    account.data[48..56].copy_from_slice(&pair.nonce.to_le_bytes());
    f.context
        .set_account(&f.proxy, &AccountSharedData::from(account));
    let account = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    let proxy = Proxy::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(proxy.consumed_pair(), pair);
}

async fn set_clock(f: &mut Fixture, timestamp: i64) {
    let mut clock: Clock = f.context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = timestamp;
    f.context.set_sysvar(&clock);
}

#[tokio::test]
async fn first_ordinary_repeated_wrong_and_clock_regression() {
    let mut f = setup().await;
    let first = vector("first");
    let ordinary = vector("ordinary");
    submit_signed(&mut f, &first, &first, Ok(pair(0, 1))).await;
    let original_now: i64 = fixture()["now"].as_str().unwrap().parse().unwrap();
    set_clock(&mut f, original_now - 60).await;
    submit_signed(&mut f, &first, &first, Err(INVALID_PAIR)).await;
    set_clock(&mut f, original_now).await;
    submit_signed(
        &mut f,
        &vector("wrong-generation"),
        &vector("wrong-generation"),
        Err(INVALID_PAIR),
    )
    .await;
    submit_signed(&mut f, &ordinary, &first, Err(INVALID_SIGNATURE)).await;
    submit_signed(&mut f, &ordinary, &ordinary, Ok(pair(0, 2))).await;
    submit_signed(&mut f, &ordinary, &ordinary, Err(INVALID_PAIR)).await;
}

#[tokio::test]
async fn max_to_zero_rejects_old_competitor_and_accepts_future_presigning() {
    let mut f = setup().await;
    set_pair(&mut f, pair(7, 0)).await;
    let initial = vector("near-wrap-cancel");
    submit_signed(&mut f, &initial, &initial, Ok(pair(7, 1))).await;
    set_pair(&mut f, pair(7, u64::MAX - 1)).await;
    let future: i64 = fixture()["futureNow"].as_str().unwrap().parse().unwrap();
    set_clock(&mut f, future).await;
    let max = vector("max");
    submit_signed(&mut f, &max, &max, Ok(pair(7, u64::MAX))).await;
    let wrong_zero = vector("wrong-zero");
    submit_signed(&mut f, &wrong_zero, &wrong_zero, Err(INVALID_PAIR)).await;
    let zero = vector("zero");
    submit_signed(&mut f, &zero, &zero, Ok(pair(8, 0))).await;
    let old = vector("near-wrap-competing");
    submit_signed(&mut f, &old, &old, Err(INVALID_PAIR)).await;
    let presigned = vector("future-presigned");
    submit_signed(&mut f, &presigned, &presigned, Ok(pair(8, 1))).await;
    submit_signed(&mut f, &old, &old, Err(INVALID_PAIR)).await;
}

#[tokio::test]
async fn terminal_generation_overflow_rejects_without_event_or_write() {
    let mut f = setup().await;
    set_pair(&mut f, pair(u64::MAX, u64::MAX)).await;
    let terminal = vector("terminal");
    submit_signed(&mut f, &terminal, &terminal, Err(GENERATION_OVERFLOW)).await;
}

#[tokio::test]
async fn independent_digest_vectors_bind_fields_and_domain() {
    let base = vector("first");
    for name in [
        "other-chain",
        "other-program",
        "other-store",
        "other-genesis",
        "other-name",
        "other-version",
        "other-proxy",
    ] {
        let mut f = setup().await;
        let variant = vector(name);
        submit_signed(&mut f, &base, &variant, Err(INVALID_SIGNATURE)).await;
    }
    let mut f = setup().await;
    submit_signed(
        &mut f,
        &vector("full-width-chain"),
        &vector("full-width-chain"),
        Ok(pair(0, 1)),
    )
    .await;
    let mut f = setup().await;
    submit_signed(
        &mut f,
        &vector("deadline-now"),
        &base,
        Err(INVALID_SIGNATURE),
    )
    .await;
}

#[tokio::test]
async fn signed_deadline_boundaries_and_negative_clock() {
    for (name, expected) in [
        ("deadline-now", Ok(pair(0, 1))),
        ("deadline-max", Ok(pair(0, 1))),
        ("deadline-past", Err(INVALID_DEADLINE)),
        ("deadline-too-far", Err(INVALID_DEADLINE)),
    ] {
        let mut f = setup().await;
        let value = vector(name);
        submit_signed(&mut f, &value, &value, expected).await;
    }
    let mut f = setup().await;
    set_clock(&mut f, -1).await;
    let first = vector("first");
    submit_signed(&mut f, &first, &first, Err(INVALID_DEADLINE)).await;
}

#[tokio::test]
async fn policy_and_signature_controls_preserve_pair() {
    let mut f = setup().await;
    let base = vector("first");
    let original = f
        .context
        .banks_client
        .get_account(f.config)
        .await
        .unwrap()
        .unwrap();
    let mut config = Config::try_deserialize(&mut original.data.as_slice()).unwrap();
    config.paused = true;
    f.context
        .set_account(&f.config, &AccountSharedData::from(config_account(&config)));
    submit_signed(&mut f, &base, &base, Err(PAUSED)).await;
    config.paused = false;
    config.allowed_chain_ids.clear();
    f.context
        .set_account(&f.config, &AccountSharedData::from(config_account(&config)));
    submit_signed(&mut f, &base, &base, Err(CHAIN_NOT_ALLOWED)).await;
    f.context
        .set_account(&f.config, &AccountSharedData::from(original));

    let secret = SecretKey::random(&mut OsRng);
    let digest = field(&base, "digest");
    let (other, recovery) = libsecp256k1::sign(&Message::parse(&digest), &secret);
    let mut ix = cancel(&f, &base, &base);
    let other = other.serialize();
    let offset = ix.data.len() - 65;
    ix.data[offset..offset + 64].copy_from_slice(&other);
    ix.data[offset + 64] = recovery.serialize();
    submit(&mut f, ix, Err(INVALID_SIGNATURE)).await;
    for signature in [[0; 64], [0xff; 64]] {
        let mut ix = cancel(&f, &base, &base);
        let offset = ix.data.len() - 65;
        ix.data[offset..offset + 64].copy_from_slice(&signature);
        submit(&mut f, ix, Err(INVALID_SIGNATURE)).await;
    }
    let mut ix = cancel(&f, &base, &base);
    *ix.data.last_mut().unwrap() = 27;
    submit(&mut f, ix, Err(INVALID_SIGNATURE)).await;
    submit_signed(&mut f, &base, &base, Ok(pair(0, 1))).await;
}

#[tokio::test]
async fn canonical_accounts_and_recorded_identity_are_required() {
    let mut f = setup().await;
    let base = vector("first");
    let config = f
        .context
        .banks_client
        .get_account(f.config)
        .await
        .unwrap()
        .unwrap();
    let alternate_config = Pubkey::new_unique();
    f.context
        .set_account(&alternate_config, &AccountSharedData::from(config));
    let mut ix = cancel(&f, &base, &base);
    ix.accounts[1].pubkey = alternate_config;
    submit(&mut f, ix, Err(2006)).await;

    let original = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    let alternate_proxy = Pubkey::new_unique();
    f.context
        .set_account(&alternate_proxy, &AccountSharedData::from(original.clone()));
    let mut ix = cancel(&f, &base, &base);
    ix.accounts[2].pubkey = alternate_proxy;
    submit(&mut f, ix, Err(2006)).await;
    for offset in [8, 9, 16] {
        let mut changed = original.clone();
        changed.data[offset] ^= 1;
        f.context
            .set_account(&f.proxy, &AccountSharedData::from(changed));
        submit_signed(&mut f, &base, &base, Err(INVALID_PROXY)).await;
    }
    f.context
        .set_account(&f.proxy, &AccountSharedData::from(original));
    submit_signed(&mut f, &base, &base, Ok(pair(0, 1))).await;
}
