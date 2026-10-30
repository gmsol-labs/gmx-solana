use anchor_lang::{AccountSerialize, Discriminator, InstructionData, ToAccountMetas};
use bytemuck::Zeroable;
use gmsol_evm_proxy::{
    accounts, instruction, Config, Proxy, CONFIG_SEED, ID, PROXY_SEED, PROXY_VERSION,
};
use libsecp256k1::{Message, PublicKey, RecoveryId, SecretKey, Signature};
use rand::rngs::OsRng;
use serde_json::Value;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::Account,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{Instruction, InstructionError},
    keccak,
    pubkey::Pubkey,
    rent::Rent,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};

const NOW: u64 = 1_700_000_000;
const DEADLINE: u64 = NOW + 120;
const PAUSED: u32 = 6008;
const CHAIN_NOT_ALLOWED: u32 = 6009;
const INVALID_DEADLINE: u32 = 6010;
const INVALID_SIGNATURE: u32 = 6011;

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    let hex = hex.strip_prefix("0x").unwrap();
    assert_eq!(hex.len(), N * 2);
    std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
}

fn vectors() -> Vec<Value> {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/verification-vectors.json")).unwrap();
    assert_eq!(fixture["generator"], "viem@2.57.7");
    assert_eq!(fixture["programAddress"], ID.to_string());
    let vectors = fixture["vectors"].as_array().unwrap().clone();
    assert_eq!(vectors.len(), 9);
    vectors
}

fn vector(name: &str) -> Value {
    vectors().into_iter().find(|v| v["name"] == name).unwrap()
}

fn field<const N: usize>(vector: &Value, name: &str) -> [u8; N] {
    bytes(vector[name].as_str().unwrap())
}

fn address(public: &PublicKey) -> [u8; 20] {
    keccak::hash(&public.serialize()[1..]).as_ref()[12..]
        .try_into()
        .unwrap()
}

fn signed(vector: &Value, secret: &SecretKey) -> ([u8; 64], u8) {
    let (signature, recovery) =
        libsecp256k1::sign(&Message::parse(&field(vector, "digest")), secret);
    (signature.serialize(), recovery.serialize())
}

fn account(data: Vec<u8>) -> Account {
    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: ID,
        executable: false,
        rent_epoch: 0,
    }
}

fn config_account(config: &Config) -> Account {
    let mut data = Vec::new();
    config.try_serialize(&mut data).unwrap();
    account(data)
}

fn proxy_account(evm_address: [u8; 20], bump: u8) -> Account {
    let mut proxy = Proxy::zeroed();
    proxy.version = PROXY_VERSION;
    proxy.bump = bump;
    proxy.evm_address = evm_address;
    let mut data = Proxy::DISCRIMINATOR.to_vec();
    data.extend_from_slice(bytemuck::bytes_of(&proxy));
    account(data)
}

struct Fixture {
    context: ProgramTestContext,
    config: Pubkey,
    proxy: Pubkey,
    address: [u8; 20],
    submissions: u32,
}

async fn setup(evm_address: [u8; 20]) -> Fixture {
    let base = vector("base");
    let (proxy, bump) = Pubkey::find_program_address(&[PROXY_SEED, &evm_address], &ID);
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
    let mut program = ProgramTest::default();
    program.prefer_bpf(true);
    program.add_program("gmsol_evm_proxy", ID, None);
    program.add_account(config, config_account(&value));
    program.add_account(proxy, proxy_account(evm_address, bump));
    let context = program.start_with_context().await;
    let mut clock: Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = NOW.try_into().unwrap();
    context.set_sysvar(&clock);
    Fixture {
        context,
        config,
        proxy,
        address: evm_address,
        submissions: 0,
    }
}

fn probe(f: &Fixture, vector: &Value, signature: [u8; 64], recovery_id: u8) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::VerifyIntentProbe {
            config: f.config,
            proxy: f.proxy,
        }
        .to_account_metas(None),
        data: instruction::VerifyIntentProbe {
            evm_address: f.address,
            chain_id: field(vector, "chainId"),
            deadline: DEADLINE,
            struct_hash: field(vector, "structHash"),
            signature,
            recovery_id,
        }
        .data(),
    }
}

async fn submit(f: &mut Fixture, ix: Instruction, expected_error: Option<u32>) {
    let mut before = Vec::new();
    for key in [
        f.config,
        f.proxy,
        ix.accounts[0].pubkey,
        ix.accounts[1].pubkey,
    ] {
        before.push((key, f.context.banks_client.get_account(key).await.unwrap()));
    }
    // Distinct transactions execute even when the signed probe is repeated.
    f.submissions += 1;
    let budget = ComputeBudgetInstruction::set_compute_unit_limit(200_000 + f.submissions);
    let blockhash = f.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[budget, ix],
        Some(&f.context.payer.pubkey()),
        &[&f.context.payer],
        blockhash,
    );
    let result = f.context.banks_client.process_transaction(tx).await;
    match expected_error {
        None => result.unwrap(),
        Some(code) => assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(1, InstructionError::Custom(code))
        ),
    }
    for (key, account) in before {
        assert_eq!(
            f.context.banks_client.get_account(key).await.unwrap(),
            account
        );
    }
}

#[tokio::test]
async fn independent_viem_domains_and_struct_hash_bind_signatures() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    for value in vectors()
        .into_iter()
        .filter(|v| v["name"] != "full-width-chain")
    {
        let (signature, recovery) = signed(&value, &secret);
        let input = if value["name"] == "other-request" {
            &value
        } else {
            &base
        };
        let ix = probe(&f, input, signature, recovery);
        let error = (!value["accepts"].as_bool().unwrap()).then_some(INVALID_SIGNATURE);
        submit(&mut f, ix, error).await;
    }
    let (signature, recovery) = signed(&base, &secret);
    let ix = probe(&f, &vector("other-request"), signature, recovery);
    submit(&mut f, ix, Some(INVALID_SIGNATURE)).await;
}

#[tokio::test]
async fn full_width_chain_is_allowed_and_signed_in_full() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let full = vector("full-width-chain");
    let (signature, recovery) = signed(&full, &secret);
    let ix = probe(&f, &full, signature, recovery);
    submit(&mut f, ix, None).await;
    let (signature, recovery) = signed(&vector("base"), &secret);
    let ix = probe(&f, &full, signature, recovery);
    submit(&mut f, ix, Some(INVALID_SIGNATURE)).await;
}

#[tokio::test]
async fn deadline_endpoints_and_adjacent_seconds_are_checked() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    let (signature, recovery) = signed(&base, &secret);
    for (deadline, error) in [
        (NOW, None),
        (NOW + 3600, None),
        (NOW - 1, Some(INVALID_DEADLINE)),
        (NOW + 3601, Some(INVALID_DEADLINE)),
    ] {
        let mut ix = probe(&f, &base, signature, recovery);
        // Deadline is a separate policy input in this no-effect probe, not a signed field.
        let offset = 8 + 20 + 32;
        ix.data[offset..offset + 8].copy_from_slice(&deadline.to_le_bytes());
        submit(&mut f, ix, error).await;
    }
    let mut clock: Clock = f.context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = -1;
    f.context.set_sysvar(&clock);
    let ix = probe(&f, &base, signature, recovery);
    submit(&mut f, ix, Some(INVALID_DEADLINE)).await;
}

#[tokio::test]
async fn exact_low_s_boundary_and_both_recovery_parities() {
    let base = vector("base");
    // floor(n/2), derived from SEC 2's secp256k1 order, not from the verifier constant.
    let half = bytes::<32>("0x7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0");
    let mut above = half;
    above[31] += 1;
    for (s, error) in [(half, None), (above, Some(INVALID_SIGNATURE))] {
        for recovery in [0, 1] {
            let mut signature = [0; 64];
            signature[31] = 1;
            signature[32..].copy_from_slice(&s);
            let parsed = Signature::parse_standard(&signature).unwrap();
            let public = libsecp256k1::recover(
                &Message::parse(&field(&base, "digest")),
                &parsed,
                &RecoveryId::parse(recovery).unwrap(),
            )
            .unwrap();
            assert!(libsecp256k1::verify(
                &Message::parse(&field(&base, "digest")),
                &parsed,
                &public
            ));
            let mut f = setup(address(&public)).await;
            let ix = probe(&f, &base, signature, recovery);
            submit(&mut f, ix, error).await;
        }
    }
}

#[tokio::test]
async fn recovery_id_and_malformed_signatures_reject_with_valid_control() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    let (signature, recovery) = signed(&base, &secret);
    let ix = probe(&f, &base, signature, recovery);
    submit(&mut f, ix, None).await;
    for invalid in [2, 3, 27, 28, 255] {
        let ix = probe(&f, &base, signature, invalid);
        submit(&mut f, ix, Some(INVALID_SIGNATURE)).await;
    }
    let other = SecretKey::random(&mut OsRng);
    let (other_signature, other_recovery) = signed(&base, &other);
    let ix = probe(&f, &base, other_signature, other_recovery);
    submit(&mut f, ix, Some(INVALID_SIGNATURE)).await;
    for malformed in [[0; 64], [0xff; 64]] {
        let ix = probe(&f, &base, malformed, recovery);
        submit(&mut f, ix, Some(INVALID_SIGNATURE)).await;
    }
}

#[tokio::test]
async fn policy_remove_reallow_and_pause_unpause_preserve_accounts() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    let (signature, recovery) = signed(&base, &secret);
    let mut value = Config {
        store: Pubkey::new_from_array(field(&base, "store")),
        genesis_hash: field(&base, "genesis"),
        admin: Pubkey::new_unique(),
        pending_admin: None,
        paused: false,
        allowed_chain_ids: vec![field(&base, "chainId")],
    };
    for (paused, allowed, error) in [
        (false, true, None),
        (false, false, Some(CHAIN_NOT_ALLOWED)),
        (false, true, None),
        (true, true, Some(PAUSED)),
        (false, true, None),
    ] {
        value.paused = paused;
        value.allowed_chain_ids = if allowed {
            vec![field(&base, "chainId")]
        } else {
            vec![]
        };
        f.context
            .set_account(&f.config, &config_account(&value).into());
        let ix = probe(&f, &base, signature, recovery);
        submit(&mut f, ix, error).await;
    }
}

#[tokio::test]
async fn valid_but_noncanonical_accounts_fail_seed_constraints() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    let (signature, recovery) = signed(&base, &secret);
    let other_config = Pubkey::new_unique();
    let value = f
        .context
        .banks_client
        .get_account(f.config)
        .await
        .unwrap()
        .unwrap();
    f.context.set_account(&other_config, &value.into());
    let mut ix = probe(&f, &base, signature, recovery);
    ix.accounts[0].pubkey = other_config;
    submit(&mut f, ix, Some(2006)).await;
    let other_proxy = Pubkey::new_unique();
    let value = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    f.context.set_account(&other_proxy, &value.into());
    let mut ix = probe(&f, &base, signature, recovery);
    ix.accounts[1].pubkey = other_proxy;
    submit(&mut f, ix, Some(2006)).await;
}

#[tokio::test]
async fn recorded_proxy_identity_is_checked_and_repeated_probe_has_no_effect() {
    let secret = SecretKey::random(&mut OsRng);
    let mut f = setup(address(&PublicKey::from_secret_key(&secret))).await;
    let base = vector("base");
    let (signature, recovery) = signed(&base, &secret);
    let original = f
        .context
        .banks_client
        .get_account(f.proxy)
        .await
        .unwrap()
        .unwrap();
    // Serialized offsets after the discriminator: version, bump, EVM address.
    for offset in [8, 9, 16] {
        let mut changed = original.clone();
        changed.data[offset] ^= 1;
        f.context.set_account(&f.proxy, &changed.into());
        let ix = probe(&f, &base, signature, recovery);
        submit(&mut f, ix, Some(6000)).await;
    }
    f.context.set_account(&f.proxy, &original.into());
    for _ in 0..2 {
        let ix = probe(&f, &base, signature, recovery);
        submit(&mut f, ix, None).await;
    }
}
