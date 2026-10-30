use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use gmsol_evm_proxy::{accounts, instruction, IntentPair, Proxy, ID, PROXY_SEED, PROXY_VERSION};
use solana_accounts_db::accounts_index::ScanConfig;
use solana_program_test::ProgramTest;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::{Account, AccountSharedData, ReadableAccount},
    genesis_config::GenesisConfig,
    instruction::{AccountMeta, Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};

fn create_instruction(payer: Pubkey, proxy: Pubkey, evm_address: [u8; 20]) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CreateProxy {
            payer,
            proxy,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::CreateProxy { evm_address }.data(),
    }
}

fn test_program() -> ProgramTest {
    let mut test = ProgramTest::default();
    test.prefer_bpf(true);
    test.add_program("gmsol_evm_proxy", ID, None);
    test
}

#[tokio::test]
async fn any_payer_can_create_proxy_for_an_evm_address() {
    let context = test_program().start_with_context().await;
    let evm_address = [0x23; 20];
    let (proxy, bump) =
        solana_sdk::pubkey::Pubkey::find_program_address(&[PROXY_SEED, &evm_address], &ID);
    let instruction = create_instruction(context.payer.pubkey(), proxy, evm_address);
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .expect("proxy creation succeeds");

    let account = context
        .banks_client
        .get_account(proxy)
        .await
        .expect("account fetch succeeds")
        .expect("proxy account exists");
    assert_eq!(account.owner, ID);
    assert_eq!(account.data.len(), 8 + std::mem::size_of::<Proxy>());
    assert_eq!(&account.data[..8], Proxy::DISCRIMINATOR);
    let proxy_data =
        Proxy::try_deserialize(&mut account.data.as_slice()).expect("proxy account decodes");
    assert_eq!(proxy_data.version, PROXY_VERSION);
    assert_eq!(proxy_data.bump, bump);
    assert_eq!(proxy_data.evm_address, evm_address);
    assert_eq!(proxy_data.reserved, [0; 256]);

    let bank = Bank::new_for_tests(&GenesisConfig::default());
    bank.store_account(&proxy, &AccountSharedData::from(account));
    let scanned = bank
        .get_program_accounts(&ID, &ScanConfig::default())
        .unwrap();
    assert_eq!(scanned.len(), 1);
    let (scanned_key, scanned_account) = &scanned[0];
    let indexed = Proxy::try_deserialize(&mut scanned_account.data()).unwrap();
    let (reverse_key, reverse_bump) =
        Pubkey::find_program_address(&[PROXY_SEED, &indexed.evm_address], &ID);
    assert_eq!(*scanned_key, reverse_key);
    assert_eq!(indexed.bump, reverse_bump);
}

#[tokio::test]
async fn distinct_addresses_and_payers_create_distinct_proxies() {
    let second_payer = Keypair::new();
    let mut program = test_program();
    program.add_account(
        second_payer.pubkey(),
        Account {
            lamports: 1_000_000_000,
            data: Vec::new(),
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let context = program.start_with_context().await;
    let first_address = [0x23; 20];
    let second_address = [0x45; 20];
    let (first_proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &first_address], &ID);
    let (second_proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &second_address], &ID);
    assert_ne!(first_proxy, second_proxy);

    for (payer, address, proxy) in [
        (&context.payer, first_address, first_proxy),
        (&second_payer, second_address, second_proxy),
    ] {
        let transaction = Transaction::new_signed_with_payer(
            &[create_instruction(payer.pubkey(), proxy, address)],
            Some(&payer.pubkey()),
            &[payer],
            context.last_blockhash,
        );
        context
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap();
        let account = context
            .banks_client
            .get_account(proxy)
            .await
            .unwrap()
            .unwrap();
        let proxy_data = Proxy::try_deserialize(&mut account.data.as_slice()).unwrap();
        assert_eq!(proxy_data.evm_address, address);
        assert_eq!(account.owner, ID);
    }
}

#[tokio::test]
async fn another_payer_cannot_reset_an_existing_proxy() {
    let second_payer = Keypair::new();
    let mut program = test_program();
    program.add_account(
        second_payer.pubkey(),
        Account {
            lamports: 1_000_000_000,
            data: Vec::new(),
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let mut context = program.start_with_context().await;
    let address = [0x23; 20];
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
    let first = Transaction::new_signed_with_payer(
        &[create_instruction(context.payer.pubkey(), proxy, address)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(first)
        .await
        .unwrap();
    let mut account = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();
    account.data[40..48].copy_from_slice(&7_u64.to_le_bytes());
    account.data[48..56].copy_from_slice(&42_u64.to_le_bytes());
    account.data[56] = 0x5a;
    let before = account.clone();
    context.set_account(&proxy, &AccountSharedData::from(account));

    let repeat = Transaction::new_signed_with_payer(
        &[create_instruction(second_payer.pubkey(), proxy, address)],
        Some(&second_payer.pubkey()),
        &[&second_payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(repeat)
        .await
        .unwrap();
    let after = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(after.data.len(), 296);
    assert_eq!(
        u64::from_le_bytes(after.data[40..48].try_into().unwrap()),
        7
    );
    assert_eq!(
        u64::from_le_bytes(after.data[48..56].try_into().unwrap()),
        42
    );
    let decoded = Proxy::try_deserialize(&mut after.data.as_slice()).unwrap();
    assert_eq!(decoded.evm_address, address);
    assert_eq!(
        decoded.consumed_pair(),
        IntentPair {
            generation: 7,
            nonce: 42
        }
    );
}

#[tokio::test]
async fn forged_pda_cannot_be_created() {
    let context = test_program().start_with_context().await;
    let address = [0x23; 20];
    let forged_proxy = Pubkey::new_unique();
    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(
            context.payer.pubkey(),
            forged_proxy,
            address,
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    assert!(context
        .banks_client
        .process_transaction(transaction)
        .await
        .is_err());
    assert!(context
        .banks_client
        .get_account(forged_proxy)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn existing_proxy_with_wrong_identity_is_rejected() {
    for offset in [8, 16] {
        let mut context = test_program().start_with_context().await;
        let address = [0x23; 20];
        let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
        let first = Transaction::new_signed_with_payer(
            &[create_instruction(context.payer.pubkey(), proxy, address)],
            Some(&context.payer.pubkey()),
            &[&context.payer],
            context.last_blockhash,
        );
        context
            .banks_client
            .process_transaction(first)
            .await
            .unwrap();
        let mut account = context
            .banks_client
            .get_account(proxy)
            .await
            .unwrap()
            .unwrap();
        account.data[offset] ^= 1;
        let before = account.data.clone();
        context.set_account(&proxy, &AccountSharedData::from(account));
        context.warp_to_slot(2).unwrap();
        let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

        let repeat = Transaction::new_signed_with_payer(
            &[create_instruction(context.payer.pubkey(), proxy, address)],
            Some(&context.payer.pubkey()),
            &[&context.payer],
            blockhash,
        );
        assert!(context
            .banks_client
            .process_transaction(repeat)
            .await
            .is_err());
        let after = context
            .banks_client
            .get_account(proxy)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.data, before);
    }
}

#[tokio::test]
async fn prefunded_system_account_can_be_initialized() {
    let address = [0x67; 20];
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
    let mut program = test_program();
    program.add_account(
        proxy,
        Account {
            lamports: 1_000_000,
            data: Vec::new(),
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let context = program.start_with_context().await;
    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(context.payer.pubkey(), proxy, address)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
    let account = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.owner, ID);
    let decoded = Proxy::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(decoded.evm_address, address);
}

#[tokio::test]
async fn occupied_pda_with_other_owner_is_rejected() {
    let address = [0x78; 20];
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
    let mut program = test_program();
    let occupant = Account {
        lamports: 3_000_000,
        data: vec![0x5a; 296],
        owner: Pubkey::new_unique(),
        executable: false,
        rent_epoch: 0,
    };
    program.add_account(proxy, occupant.clone());
    let context = program.start_with_context().await;
    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(context.payer.pubkey(), proxy, address)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    assert!(context
        .banks_client
        .process_transaction(transaction)
        .await
        .is_err());
    let after = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.owner, occupant.owner);
    assert_eq!(after.data, occupant.data);
    assert_eq!(after.lamports, occupant.lamports);
}

#[tokio::test]
async fn insufficient_payer_funds_leave_no_proxy() {
    let payer = Keypair::new();
    let address = [0x89; 20];
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
    let mut program = test_program();
    program.add_account(
        payer.pubkey(),
        Account {
            lamports: 100_000,
            data: Vec::new(),
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let context = program.start_with_context().await;
    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(payer.pubkey(), proxy, address)],
        Some(&payer.pubkey()),
        &[&payer],
        context.last_blockhash,
    );
    assert!(context
        .banks_client
        .process_transaction(transaction)
        .await
        .is_err());
    assert!(context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn creator_cannot_sign_for_proxy_or_dispatch_a_mutation() {
    let context = test_program().start_with_context().await;
    let address = [0xab; 20];
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &address], &ID);
    let create = Transaction::new_signed_with_payer(
        &[create_instruction(context.payer.pubkey(), proxy, address)],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(create)
        .await
        .unwrap();
    let before = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();

    let mut close_attempt = Transaction::new_with_payer(
        &[solana_sdk::system_instruction::transfer(
            &proxy,
            &context.payer.pubkey(),
            1,
        )],
        Some(&context.payer.pubkey()),
    );
    assert!(close_attempt
        .try_sign(&[&context.payer], context.last_blockhash)
        .is_err());

    let unknown_instruction = Instruction {
        program_id: ID,
        accounts: vec![AccountMeta::new(proxy, false)],
        data: vec![0xff; 8],
    };
    let transaction = Transaction::new_signed_with_payer(
        &[unknown_instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    let error = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err();
    assert_eq!(
        error.unwrap(),
        TransactionError::InstructionError(0, InstructionError::Custom(101))
    );
    let after = context
        .banks_client
        .get_account(proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn proxy_idl_retains_create_instruction() {
    let idl: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/programs/idls/gmsol_evm_proxy.json"
    ))
    .unwrap();
    let instructions = idl["instructions"].as_array().unwrap();
    let create = instructions
        .iter()
        .find(|instruction| instruction["name"] == "create_proxy")
        .expect("create_proxy remains in the IDL");
    assert_eq!(create["args"][0]["name"], "evm_address");
}
