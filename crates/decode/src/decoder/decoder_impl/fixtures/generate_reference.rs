// Run in the isolated reference project described in README.md, not this workspace.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use solana_message::{v1, MessageHeader, VersionedMessage};
use solana_transaction::versioned::VersionedTransaction;

fn fixture(name: String, message: v1::Message) -> Value {
    let signatures: Vec<_> = (0..message.header.num_required_signatures)
        .map(|index| [index + 10; 64].into())
        .collect();
    let tx = VersionedTransaction {
        signatures,
        message: VersionedMessage::V1(message.clone()),
    };
    tx.sanitize().unwrap();
    let bytes = wincode::serialize(&tx).unwrap();
    assert!(bytes.len() <= v1::MAX_TRANSACTION_SIZE);
    let decoded: VersionedTransaction = wincode::deserialize(&bytes).unwrap();
    assert_eq!(tx, decoded);
    json!({
        "name": name, "wire": STANDARD.encode(bytes),
        "header": [message.header.num_required_signatures,
            message.header.num_readonly_signed_accounts,
            message.header.num_readonly_unsigned_accounts],
        "config": {
            "priority_fee": message.config.priority_fee,
            "compute_unit_limit": message.config.compute_unit_limit,
            "loaded_accounts_data_size_limit": message.config.loaded_accounts_data_size_limit,
            "heap_size": message.config.heap_size,
        },
        "lifetime": message.lifetime_specifier.to_string(),
        "accounts": message.account_keys.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "signatures": tx.signatures.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "instructions": message.instructions.iter().map(|ix| json!({
            "program": ix.program_id_index, "accounts": ix.accounts,
            "data": STANDARD.encode(&ix.data),
        })).collect::<Vec<_>>()
    })
}

fn main() {
    let base = v1::Message {
        header: MessageHeader {
            num_required_signatures: 3,
            num_readonly_signed_accounts: 1,
            num_readonly_unsigned_accounts: 1,
        },
        config: v1::TransactionConfig::empty(),
        lifetime_specifier: [8; 32].into(),
        account_keys: (1..=6).map(|byte| [byte; 32].into()).collect(),
        instructions: vec![
            solana_message::compiled_instruction::CompiledInstruction {
                program_id_index: 5,
                accounts: vec![0, 1, 2, 3],
                data: vec![1, 2, 3],
            },
            solana_message::compiled_instruction::CompiledInstruction {
                program_id_index: 4,
                accounts: vec![],
                data: vec![0x55; 17],
            },
        ],
    };
    let mut cases = Vec::new();
    for bits in 0..16 {
        let mut message = base.clone();
        message.config = v1::TransactionConfig {
            priority_fee: (bits & 1 != 0).then_some(50_000),
            compute_unit_limit: (bits & 2 != 0).then_some(200_000),
            loaded_accounts_data_size_limit: (bits & 4 != 0).then_some(65_536),
            heap_size: (bits & 8 != 0).then_some(65_536),
        };
        cases.push(fixture(format!("config-{bits}"), message));
    }
    let mut large = base.clone();
    large.instructions[0].data = vec![0x42; 1500];
    cases.push(fixture("large".into(), large.clone()));
    let overhead = wincode::serialize(&VersionedTransaction {
        signatures: vec![[10; 64].into(); 3],
        message: VersionedMessage::V1(large.clone()),
    })
    .unwrap()
    .len()
        - 1500;
    large.instructions[0].data.resize(4096 - overhead, 0x42);
    cases.push(fixture("max-size".into(), large));
    let mut zero = base.clone();
    zero.config.priority_fee = Some(0);
    zero.config.compute_unit_limit = Some(0);
    zero.config.loaded_accounts_data_size_limit = Some(0);
    zero.config.heap_size = Some(32768);
    zero.instructions.clear();
    cases.push(fixture("zero-values-no-instructions".into(), zero));
    let mut limits = base;
    limits.header.num_required_signatures = 12;
    limits.account_keys = (1..=64).map(|byte| [byte; 32].into()).collect();
    limits.instructions = vec![
        solana_message::compiled_instruction::CompiledInstruction {
            program_id_index: 63,
            accounts: vec![0, 62],
            data: vec![],
        };
        64
    ];
    limits.config.heap_size = Some(262144);
    cases.push(fixture("max-counts".into(), limits));
    // Keep one complete case per line, matching the checked-in fixture format.
    let lines: Vec<_> = cases
        .iter()
        .map(|case| serde_json::to_string(case).unwrap())
        .collect();
    println!("[\n{}\n]", lines.join(",\n"));
}
