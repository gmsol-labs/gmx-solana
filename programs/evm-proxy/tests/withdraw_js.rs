use std::{
    io::Write,
    process::{Command, Stdio},
};

use anchor_lang::{AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas};
use base64::Engine;
use gmsol_evm_proxy::{
    accounts, instruction, Config, IntentPair, Proxy, CONFIG_SEED, ID, PROXY_SEED,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account,
    clock::Clock,
    hash::Hash,
    message::VersionedMessage,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, VersionedTransaction},
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

const NOW: i64 = 1_700_000_000;
const GENESIS: [u8; 32] = [42; 32];

fn account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: Rent::default().minimum_balance(data.len()).max(1),
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Account {
    let state = TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    let mut data = vec![0; TokenAccount::LEN];
    TokenAccount::pack(state, &mut data).unwrap();
    account(spl_token::id(), data)
}

#[tokio::test]
async fn generated_js_transaction_executes_on_production_sbf() {
    let emitter = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/anchor_test/scripts/emit-js-signed-withdraw.cjs"
    );
    let external_payer = Keypair::new();
    let store = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let (config, _) = Pubkey::find_program_address(&[CONFIG_SEED], &ID);
    let mut chain = [0; 32];
    chain[31] = 1;
    let input = serde_json::json!({
        "payer": external_payer.pubkey().to_string(),
        "blockhash": Hash::default().to_string(),
        "intent": {
            "chainId": format!("0x{}", chain.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            "mint": mint.to_string(), "amount": "125", "decimals": 6,
            "recipient": recipient.to_string(), "generation": "0", "nonce": "1",
            "deadline": (NOW + 120).to_string(), "store": store.to_string(),
            "genesisHash": format!("0x{}", GENESIS.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            "programId": ID.to_string(),
        },
    });
    let mut child = Command::new("node")
        .arg(emitter)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let emitted: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let address = emitted["evmAddress"]
        .as_str()
        .unwrap()
        .trim_start_matches("0x");
    assert_eq!(address.len(), 40);
    let evm_address: [u8; 20] =
        std::array::from_fn(|i| u8::from_str_radix(&address[i * 2..i * 2 + 2], 16).unwrap());
    let (proxy, _) = Pubkey::find_program_address(&[PROXY_SEED, &evm_address], &ID);
    let source = anchor_spl::associated_token::get_associated_token_address_with_program_id(
        &proxy,
        &mint,
        &spl_token::id(),
    );
    let destination = anchor_spl::associated_token::get_associated_token_address_with_program_id(
        &recipient,
        &mint,
        &spl_token::id(),
    );

    let config_data = Config {
        store,
        genesis_hash: GENESIS,
        admin: Pubkey::new_unique(),
        pending_admin: None,
        paused: false,
        allowed_chain_ids: vec![chain],
    };
    let mut config_bytes = Vec::new();
    config_data.try_serialize(&mut config_bytes).unwrap();
    let mut mint_bytes = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: 500,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut mint_bytes,
    )
    .unwrap();
    let mut test = ProgramTest::default();
    test.prefer_bpf(true);
    test.add_program("gmsol_evm_proxy", ID, None);
    test.prefer_bpf(false);
    test.add_program(
        "spl_token",
        spl_token::id(),
        processor!(spl_token::processor::Processor::process),
    );
    test.add_account(config, account(ID, config_bytes));
    test.add_account(mint, account(spl_token::id(), mint_bytes));
    test.add_account(source, token_account(mint, proxy, 500));
    test.add_account(destination, token_account(mint, recipient, 10));
    test.add_account(recipient, account(system_program::id(), vec![]));
    test.add_account(
        external_payer.pubkey(),
        Account {
            lamports: 1_000_000_000,
            ..account(system_program::id(), vec![])
        },
    );
    let context = test.start_with_context().await;
    let create = solana_sdk::instruction::Instruction {
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
    let mut clock: Clock = context.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = NOW;
    context.set_sysvar(&clock);

    let wire = base64::engine::general_purpose::STANDARD
        .decode(emitted["transaction"].as_str().unwrap())
        .unwrap();
    let unsigned: VersionedTransaction = bincode::deserialize(&wire).unwrap();
    let VersionedMessage::Legacy(mut message) = unsigned.message else {
        panic!("expected legacy JS transaction")
    };
    assert_eq!(message.account_keys[0], external_payer.pubkey());
    message.recent_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let signed =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&external_payer])
            .unwrap();
    let result = context
        .banks_client
        .process_transaction_with_metadata(signed)
        .await
        .unwrap();
    assert!(result.result.is_ok(), "{:?}", result.result);
    let source = context
        .banks_client
        .get_account(source)
        .await
        .unwrap()
        .unwrap();
    let destination = context
        .banks_client
        .get_account(destination)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(TokenAccount::unpack(&source.data).unwrap().amount, 375);
    assert_eq!(TokenAccount::unpack(&destination.data).unwrap().amount, 135);
    let proxy = context
        .banks_client
        .get_account(proxy)
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
