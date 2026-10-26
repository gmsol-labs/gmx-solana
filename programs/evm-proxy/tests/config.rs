use anchor_lang::{
    AccountDeserialize, AnchorDeserialize, Discriminator, InstructionData, ToAccountMetas,
};
use base64::Engine;
use gmsol_evm_proxy::{accounts, instruction, Config, ConfigCreated, CONFIG_SEED, ID};
use gmsol_store::states::Seed;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{
    account::{Account, AccountSharedData},
    account_info::AccountInfo,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};

const GENESIS_HASH: [u8; 32] = [0x42; 32];
const GOVERNANCE_PROGRAM_ID: Pubkey = Pubkey::new_from_array([0x56; 32]);

fn governance_process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    let [admin, config, proxy_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if proxy_program.key != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (expected_admin, bump) = Pubkey::find_program_address(&[b"admin"], program_id);
    if admin.key != &expected_admin {
        return Err(ProgramError::InvalidArgument);
    }
    let ix = match input {
        [0] => Instruction {
            program_id: ID,
            accounts: accounts::AcceptConfigAdmin {
                next_admin: *admin.key,
                config: *config.key,
            }
            .to_account_metas(None),
            data: instruction::AcceptAdmin {}.data(),
        },
        [1] => Instruction {
            program_id: ID,
            accounts: accounts::CurrentConfigAdmin {
                admin: *admin.key,
                config: *config.key,
            }
            .to_account_metas(None),
            data: instruction::SetPaused { paused: false }.data(),
        },
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    invoke_signed(
        &ix,
        &[admin.clone(), config.clone(), proxy_program.clone()],
        &[&[b"admin", &[bump]]],
    )
}

fn create_instruction(
    payer: Pubkey,
    authority: Pubkey,
    config: Pubkey,
    program_data: Pubkey,
    store: Pubkey,
) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CreateConfig {
            payer,
            upgrade_authority: authority,
            config,
            program: ID,
            program_data,
            store,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::CreateConfig {
            genesis_hash: GENESIS_HASH,
            first_admin: authority,
        }
        .data(),
    }
}

async fn setup() -> (ProgramTestContext, Pubkey, Pubkey, Pubkey) {
    let mut test = ProgramTest::default();
    test.prefer_bpf(false);
    test.add_upgradeable_program_to_genesis("gmsol_evm_proxy", &ID);
    test.add_program(
        "governance_test",
        GOVERNANCE_PROGRAM_ID,
        solana_program_test::processor!(governance_process),
    );

    let store = Pubkey::new_unique();
    let mut store_data = vec![0; 8 + std::mem::size_of::<gmsol_store::states::Store>()];
    store_data[..8].copy_from_slice(gmsol_store::states::Store::DISCRIMINATOR);
    test.add_account(
        store,
        Account {
            lamports: Rent::default().minimum_balance(store_data.len()),
            data: store_data,
            owner: gmsol_store::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut context = test.start_with_context().await;
    let program_data = bpf_loader_upgradeable::get_program_data_address(&ID);
    let mut account = context
        .banks_client
        .get_account(program_data)
        .await
        .unwrap()
        .unwrap();
    let header = bincode::serialize(&UpgradeableLoaderState::ProgramData {
        slot: 0,
        upgrade_authority_address: Some(context.payer.pubkey()),
    })
    .unwrap();
    account.data[..header.len()].copy_from_slice(&header);
    context.set_account(&program_data, &AccountSharedData::from(account));
    let (config, _) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
    (context, store, program_data, config)
}

#[tokio::test]
async fn binds_store_initialized_by_store_program() {
    let mut test = ProgramTest::default();
    test.prefer_bpf(true);
    test.add_upgradeable_program_to_genesis("gmsol_evm_proxy", &ID);
    test.add_program("gmsol_store", gmsol_store::ID, None);

    let mut context = test.start_with_context().await;
    let program_data = bpf_loader_upgradeable::get_program_data_address(&ID);
    let mut account = context
        .banks_client
        .get_account(program_data)
        .await
        .unwrap()
        .unwrap();
    let header = bincode::serialize(&UpgradeableLoaderState::ProgramData {
        slot: 0,
        upgrade_authority_address: Some(context.payer.pubkey()),
    })
    .unwrap();
    account.data[..header.len()].copy_from_slice(&header);
    context.set_account(&program_data, &AccountSharedData::from(account));

    let store_seed = gmsol_utils::to_seed("");
    let (store, _) = Pubkey::find_program_address(
        &[gmsol_store::states::Store::SEED, &store_seed],
        &gmsol_store::ID,
    );
    let payer = context.payer.pubkey();
    let initialize_store = Instruction {
        program_id: gmsol_store::ID,
        accounts: gmsol_store::accounts::Initialize {
            payer,
            authority: None,
            receiver: None,
            holding: None,
            store,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: gmsol_store::instruction::Initialize { key: String::new() }.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[initialize_store],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
    let initialized = context
        .banks_client
        .get_account(store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(initialized.owner, gmsol_store::ID);

    let (config, _) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
    let create = create_instruction(payer, payer, config, program_data, store);
    let transaction = Transaction::new_signed_with_payer(
        &[create],
        Some(&payer),
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
        .get_account(config)
        .await
        .unwrap()
        .unwrap();
    let decoded = Config::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(decoded.store, store);
    assert_eq!(decoded.genesis_hash, GENESIS_HASH);
}

#[tokio::test]
async fn creates_and_decodes_singleton_config_and_event() {
    let (context, store, program_data, config) = setup().await;
    let authority = context.payer.pubkey();
    let create = create_instruction(authority, authority, config, program_data, store);
    let transaction = Transaction::new_signed_with_payer(
        &[create.clone()],
        Some(&authority),
        &[&context.payer],
        context.last_blockhash,
    );
    let result = context
        .banks_client
        .process_transaction_with_metadata(transaction)
        .await
        .unwrap();
    assert_eq!(result.result, Ok(()));

    let account = context
        .banks_client
        .get_account(config)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.owner, ID);
    let decoded = Config::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(decoded.store, store);
    assert_eq!(decoded.genesis_hash, GENESIS_HASH);
    assert_eq!(decoded.admin, authority);

    let events: Vec<ConfigCreated> = result
        .metadata
        .unwrap()
        .log_messages
        .iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .filter_map(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .filter(|data| data.starts_with(ConfigCreated::DISCRIMINATOR))
        .map(|data| ConfigCreated::deserialize(&mut &data[8..]).unwrap())
        .collect();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.config, config);
    assert_eq!(event.store, decoded.store);
    assert_eq!(event.genesis_hash, decoded.genesis_hash);
    assert_eq!(event.admin, decoded.admin);
    assert_eq!(event.paused, decoded.paused);
    assert_eq!(event.allowed_chain_ids, decoded.allowed_chain_ids);

    let mut repeated_create = create;
    repeated_create.data = instruction::CreateConfig {
        genesis_hash: [0x43; 32],
        first_admin: authority,
    }
    .data();
    let repeat = Transaction::new_signed_with_payer(
        &[repeated_create],
        Some(&authority),
        &[&context.payer],
        context.last_blockhash,
    );
    assert!(context
        .banks_client
        .process_transaction(repeat)
        .await
        .is_err());
    let after = context
        .banks_client
        .get_account(config)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.data, account.data);
}

#[tokio::test]
async fn rejects_wrong_authority_program_data_store_and_config() {
    let (mut context, store, program_data, config) = setup().await;
    let authority = context.payer.pubkey();
    let other = Keypair::new();
    let substituted_program_data = Pubkey::new_unique();
    let substituted_data = bincode::serialize(&UpgradeableLoaderState::ProgramData {
        slot: 0,
        upgrade_authority_address: Some(authority),
    })
    .unwrap();
    context.set_account(
        &substituted_program_data,
        &AccountSharedData::from(Account {
            lamports: Rent::default().minimum_balance(substituted_data.len()),
            data: substituted_data,
            owner: bpf_loader_upgradeable::ID,
            executable: false,
            rent_epoch: 0,
        }),
    );
    let wrong_authority =
        create_instruction(authority, other.pubkey(), config, program_data, store);
    let wrong_program_data = create_instruction(
        authority,
        authority,
        config,
        substituted_program_data,
        store,
    );
    let wrong_store = create_instruction(
        authority,
        authority,
        config,
        program_data,
        Pubkey::new_unique(),
    );
    let wrong_config = create_instruction(
        authority,
        authority,
        Pubkey::new_unique(),
        program_data,
        store,
    );

    let wrong_authority_tx = Transaction::new_signed_with_payer(
        &[wrong_authority],
        Some(&authority),
        &[&context.payer, &other],
        context.last_blockhash,
    );
    assert!(context
        .banks_client
        .process_transaction(wrong_authority_tx)
        .await
        .is_err());
    assert!(context
        .banks_client
        .get_account(config)
        .await
        .unwrap()
        .is_none());

    for instruction in [wrong_program_data, wrong_store, wrong_config] {
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority),
            &[&context.payer],
            context.last_blockhash,
        );
        let result = context
            .banks_client
            .process_transaction_with_metadata(transaction)
            .await
            .unwrap();
        assert!(result.result.is_err());
        assert!(!result
            .metadata
            .unwrap()
            .log_messages
            .iter()
            .any(|line| line.contains("panicked")));
        assert!(context
            .banks_client
            .get_account(config)
            .await
            .unwrap()
            .is_none());
    }

    let mut bad_store = context
        .banks_client
        .get_account(store)
        .await
        .unwrap()
        .unwrap();
    bad_store.data[..8].fill(0);
    context.set_account(&store, &AccountSharedData::from(bad_store));
    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(
            authority,
            authority,
            config,
            program_data,
            store,
        )],
        Some(&authority),
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
        .get_account(config)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn revoked_upgrade_authority_cannot_create_config() {
    let (mut context, store, program_data, config) = setup().await;
    let authority = context.payer.pubkey();
    let mut account = context
        .banks_client
        .get_account(program_data)
        .await
        .unwrap()
        .unwrap();
    let header = bincode::serialize(&UpgradeableLoaderState::ProgramData {
        slot: 0,
        upgrade_authority_address: None,
    })
    .unwrap();
    account.data[..header.len()].copy_from_slice(&header);
    context.set_account(&program_data, &AccountSharedData::from(account));

    let transaction = Transaction::new_signed_with_payer(
        &[create_instruction(
            authority,
            authority,
            config,
            program_data,
            store,
        )],
        Some(&authority),
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
        .get_account(config)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn foreign_or_truncated_store_cannot_be_bound() {
    for is_foreign in [true, false] {
        let (mut context, store, program_data, config) = setup().await;
        let authority = context.payer.pubkey();
        let mut account = context
            .banks_client
            .get_account(store)
            .await
            .unwrap()
            .unwrap();
        if is_foreign {
            account.owner = system_program::ID;
        } else {
            account.data.truncate(8);
        }
        context.set_account(&store, &AccountSharedData::from(account));

        let transaction = Transaction::new_signed_with_payer(
            &[create_instruction(
                authority,
                authority,
                config,
                program_data,
                store,
            )],
            Some(&authority),
            &[&context.payer],
            context.last_blockhash,
        );
        let result = context
            .banks_client
            .process_transaction_with_metadata(transaction)
            .await
            .unwrap();
        assert!(result.result.is_err());
        assert!(!result
            .metadata
            .unwrap()
            .log_messages
            .iter()
            .any(|line| line.contains("panicked")));
        assert!(context
            .banks_client
            .get_account(config)
            .await
            .unwrap()
            .is_none());
    }
}

#[path = "config/admin.rs"]
mod admin;

#[tokio::test]
async fn zero_first_admin_is_rejected_before_creation() {
    let (context, store, program_data, config) = setup().await;
    let authority = context.payer.pubkey();
    let mut create = create_instruction(authority, authority, config, program_data, store);
    create.data = instruction::CreateConfig {
        genesis_hash: GENESIS_HASH,
        first_admin: Pubkey::default(),
    }
    .data();
    let transaction = Transaction::new_signed_with_payer(
        &[create],
        Some(&authority),
        &[&context.payer],
        context.last_blockhash,
    );
    let result = context
        .banks_client
        .process_transaction_with_metadata(transaction)
        .await
        .unwrap();
    assert!(result.result.is_err());
    assert!(result
        .metadata
        .unwrap()
        .log_messages
        .iter()
        .any(|line| line.contains("InvalidAdmin")));
    assert!(context
        .banks_client
        .get_account(config)
        .await
        .unwrap()
        .is_none());
}
