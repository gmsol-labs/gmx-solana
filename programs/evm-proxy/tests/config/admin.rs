use super::*;
use gmsol_evm_proxy::{
    ConfigAdminAccepted, ConfigAdminProposed, ConfigChainAllowedChanged, ConfigPauseChanged,
    MAX_ALLOWED_CHAINS,
};
use solana_sdk::compute_budget::ComputeBudgetInstruction;

fn chain_instruction(
    admin: Pubkey,
    config: Pubkey,
    chain_id: [u8; 32],
    allowed: bool,
) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CurrentConfigAdmin { admin, config }.to_account_metas(None),
        data: instruction::SetChainAllowed { chain_id, allowed }.data(),
    }
}

fn pause_instruction(admin: Pubkey, config: Pubkey, paused: bool) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CurrentConfigAdmin { admin, config }.to_account_metas(None),
        data: instruction::SetPaused { paused }.data(),
    }
}

fn propose_instruction(admin: Pubkey, config: Pubkey, next_admin: Pubkey) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CurrentConfigAdmin { admin, config }.to_account_metas(None),
        data: instruction::ProposeAdmin { next_admin }.data(),
    }
}

fn accept_instruction(next_admin: Pubkey, config: Pubkey) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::AcceptConfigAdmin { next_admin, config }.to_account_metas(None),
        data: instruction::AcceptAdmin {}.data(),
    }
}

async fn send(
    context: &ProgramTestContext,
    instructions: Vec<Instruction>,
    extra_signer: Option<&Keypair>,
    sequence: &mut u32,
) -> (bool, Vec<String>) {
    *sequence += 1;
    let mut instructions_with_budget = vec![ComputeBudgetInstruction::set_compute_unit_limit(
        200_000 + *sequence,
    )];
    instructions_with_budget.extend(instructions);
    let mut signers = vec![&context.payer];
    if let Some(signer) = extra_signer {
        signers.push(signer);
    }
    let transaction = Transaction::new_signed_with_payer(
        &instructions_with_budget,
        Some(&context.payer.pubkey()),
        &signers,
        context.last_blockhash,
    );
    let result = context
        .banks_client
        .process_transaction_with_metadata(transaction)
        .await
        .unwrap();
    (result.result.is_ok(), result.metadata.unwrap().log_messages)
}

async fn initialize(
    context: &ProgramTestContext,
    store: Pubkey,
    program_data: Pubkey,
    config: Pubkey,
    sequence: &mut u32,
) {
    let payer = context.payer.pubkey();
    let (ok, _) = send(
        context,
        vec![create_instruction(
            payer,
            payer,
            config,
            program_data,
            store,
        )],
        None,
        sequence,
    )
    .await;
    assert!(ok);
}

async fn read_config(context: &ProgramTestContext, config: Pubkey) -> (Vec<u8>, Config) {
    let account = context
        .banks_client
        .get_account(config)
        .await
        .unwrap()
        .unwrap();
    let decoded = Config::try_deserialize(&mut account.data.as_slice()).unwrap();
    (account.data, decoded)
}

fn events<E: AnchorDeserialize + Discriminator>(logs: &[String]) -> Vec<E> {
    logs.iter()
        .filter_map(|line| line.strip_prefix("Program data: "))
        .filter_map(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .filter(|data| data.starts_with(E::DISCRIMINATOR))
        .map(|data| E::deserialize(&mut &data[E::DISCRIMINATOR.len()..]).unwrap())
        .collect()
}

fn chain_id(high: u8, low: u8) -> [u8; 32] {
    let mut id = [0; 32];
    id[0] = high;
    id[31] = low;
    id
}

#[tokio::test]
async fn governance_pda_accepts_and_administers_through_signed_cpi() {
    let (context, store, program_data, config) = setup().await;
    let mut sequence = 0;
    initialize(&context, store, program_data, config, &mut sequence).await;
    let former_admin = context.payer.pubkey();
    let (governance_admin, _) = Pubkey::find_program_address(&[b"admin"], &GOVERNANCE_PROGRAM_ID);
    let (ok, _) = send(
        &context,
        vec![propose_instruction(former_admin, config, governance_admin)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);

    let governance_instruction = |action| Instruction {
        program_id: GOVERNANCE_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(governance_admin, false),
            AccountMeta::new(config, false),
            AccountMeta::new_readonly(ID, false),
        ],
        data: vec![action],
    };
    let (ok, logs) = send(
        &context,
        vec![governance_instruction(0)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let accepted = events::<ConfigAdminAccepted>(&logs);
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].admin, governance_admin);
    let (_, state) = read_config(&context, config).await;
    assert_eq!(state.admin, governance_admin);
    assert_eq!(state.pending_admin, None);

    let (ok, _) = send(
        &context,
        vec![pause_instruction(former_admin, config, false)],
        None,
        &mut sequence,
    )
    .await;
    assert!(!ok);
    let (ok, logs) = send(
        &context,
        vec![governance_instruction(1)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let paused = events::<ConfigPauseChanged>(&logs);
    assert_eq!(paused.len(), 1);
    assert!(!paused[0].paused);
    let (_, state) = read_config(&context, config).await;
    assert!(!state.paused);
}

#[tokio::test]
async fn admin_changes_full_width_chains_and_pause_with_decodable_events() {
    let (context, store, program_data, config) = setup().await;
    let mut sequence = 0;
    initialize(&context, store, program_data, config, &mut sequence).await;
    let admin = context.payer.pubkey();
    let first = chain_id(1, 7);
    let second = chain_id(2, 7);

    for id in [first, second] {
        let (ok, logs) = send(
            &context,
            vec![chain_instruction(admin, config, id, true)],
            None,
            &mut sequence,
        )
        .await;
        assert!(ok);
        let event = events::<ConfigChainAllowedChanged>(&logs);
        assert_eq!(event.len(), 1);
        assert_eq!(
            (event[0].config, event[0].chain_id, event[0].allowed),
            (config, id, true)
        );
    }
    let (_, state) = read_config(&context, config).await;
    assert_eq!(state.allowed_chain_ids, vec![first, second]);

    let (ok, logs) = send(
        &context,
        vec![pause_instruction(admin, config, false)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let event = events::<ConfigPauseChanged>(&logs);
    assert_eq!(event.len(), 1);
    assert_eq!(
        (event[0].config, event[0].previous_paused, event[0].paused),
        (config, true, false)
    );

    let (ok, logs) = send(
        &context,
        vec![chain_instruction(admin, config, first, false)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let event = events::<ConfigChainAllowedChanged>(&logs);
    assert_eq!(event.len(), 1);
    assert_eq!((event[0].chain_id, event[0].allowed), (first, false));
    let (_, state) = read_config(&context, config).await;
    assert_eq!(state.allowed_chain_ids, vec![second]);
    assert!(!state.paused);
    assert_eq!(state.store, store);
    assert_eq!(state.genesis_hash, GENESIS_HASH);
}

#[tokio::test]
async fn pending_replacement_and_acceptance_switch_only_the_current_admin() {
    let (context, store, program_data, config) = setup().await;
    let mut sequence = 0;
    initialize(&context, store, program_data, config, &mut sequence).await;
    let old_admin = context.payer.pubkey();
    let first = Keypair::new();
    let second = Keypair::new();

    let (ok, logs) = send(
        &context,
        vec![propose_instruction(old_admin, config, first.pubkey())],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let proposed = events::<ConfigAdminProposed>(&logs);
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0].config, config);
    assert_eq!(proposed[0].admin, old_admin);
    assert_eq!(proposed[0].previous_pending_admin, None);
    assert_eq!(proposed[0].next_admin, first.pubkey());

    let (_, before) = read_config(&context, config).await;
    assert_eq!(before.admin, old_admin);
    assert_eq!(before.pending_admin, Some(first.pubkey()));
    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![propose_instruction(old_admin, config, first.pubkey())],
        None,
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert!(logs.iter().any(|line| line.contains("NoConfigChange")));
    assert!(events::<ConfigAdminProposed>(&logs).is_empty());
    assert_eq!(read_config(&context, config).await.0, before_bytes);
    let (ok, _) = send(
        &context,
        vec![pause_instruction(old_admin, config, false)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);

    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![pause_instruction(first.pubkey(), config, true)],
        Some(&first),
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert!(events::<ConfigPauseChanged>(&logs).is_empty());
    assert_eq!(read_config(&context, config).await.0, before_bytes);

    let (ok, logs) = send(
        &context,
        vec![propose_instruction(old_admin, config, second.pubkey())],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let proposed = events::<ConfigAdminProposed>(&logs);
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0].previous_pending_admin, Some(first.pubkey()));
    assert_eq!(proposed[0].next_admin, second.pubkey());

    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![accept_instruction(first.pubkey(), config)],
        Some(&first),
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert!(events::<ConfigAdminAccepted>(&logs).is_empty());
    assert_eq!(read_config(&context, config).await.0, before_bytes);

    let (ok, logs) = send(
        &context,
        vec![accept_instruction(second.pubkey(), config)],
        Some(&second),
        &mut sequence,
    )
    .await;
    assert!(ok);
    let accepted = events::<ConfigAdminAccepted>(&logs);
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].config, config);
    assert_eq!(accepted[0].previous_admin, old_admin);
    assert_eq!(accepted[0].admin, second.pubkey());
    assert_eq!(accepted[0].pending_admin, None);
    let (_, state) = read_config(&context, config).await;
    assert_eq!(state.admin, second.pubkey());
    assert_eq!(state.pending_admin, None);
    assert_eq!(state.store, store);
    assert_eq!(state.genesis_hash, GENESIS_HASH);

    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![accept_instruction(second.pubkey(), config)],
        Some(&second),
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert!(events::<ConfigAdminAccepted>(&logs).is_empty());
    assert_eq!(read_config(&context, config).await.0, before_bytes);

    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![pause_instruction(old_admin, config, true)],
        None,
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert!(events::<ConfigPauseChanged>(&logs).is_empty());
    assert_eq!(read_config(&context, config).await.0, before_bytes);
    let (ok, _) = send(
        &context,
        vec![pause_instruction(second.pubkey(), config, true)],
        Some(&second),
        &mut sequence,
    )
    .await;
    assert!(ok);
    assert!(read_config(&context, config).await.1.paused);
}

#[tokio::test]
async fn capacity_noops_and_invalid_admin_destinations_leave_state_unchanged() {
    let (context, store, program_data, config) = setup().await;
    let mut sequence = 0;
    initialize(&context, store, program_data, config, &mut sequence).await;
    let admin = context.payer.pubkey();

    for index in 0..MAX_ALLOWED_CHAINS {
        let (ok, _) = send(
            &context,
            vec![chain_instruction(
                admin,
                config,
                chain_id(9, index as u8),
                true,
            )],
            None,
            &mut sequence,
        )
        .await;
        assert!(ok);
    }
    assert_eq!(
        read_config(&context, config)
            .await
            .1
            .allowed_chain_ids
            .len(),
        MAX_ALLOWED_CHAINS
    );
    let (before_bytes, _) = read_config(&context, config).await;
    for (instruction, expected_error) in [
        (
            chain_instruction(admin, config, chain_id(9, 40), true),
            "AllowedChainsFull",
        ),
        (
            chain_instruction(admin, config, chain_id(9, 0), true),
            "NoConfigChange",
        ),
        (
            chain_instruction(admin, config, chain_id(9, 40), false),
            "NoConfigChange",
        ),
        (pause_instruction(admin, config, true), "NoConfigChange"),
        (
            propose_instruction(admin, config, Pubkey::default()),
            "InvalidAdmin",
        ),
        (propose_instruction(admin, config, admin), "NoConfigChange"),
    ] {
        let (ok, logs) = send(&context, vec![instruction], None, &mut sequence).await;
        assert!(!ok);
        assert!(logs.iter().any(|line| line.contains(expected_error)));
        assert!(events::<ConfigChainAllowedChanged>(&logs).is_empty());
        assert!(events::<ConfigPauseChanged>(&logs).is_empty());
        assert!(events::<ConfigAdminProposed>(&logs).is_empty());
        assert_eq!(read_config(&context, config).await.0, before_bytes);
    }

    let (ok, _) = send(
        &context,
        vec![chain_instruction(admin, config, chain_id(9, 0), false)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    let (ok, _) = send(
        &context,
        vec![chain_instruction(admin, config, chain_id(9, 40), true)],
        None,
        &mut sequence,
    )
    .await;
    assert!(ok);
    assert_eq!(
        read_config(&context, config)
            .await
            .1
            .allowed_chain_ids
            .len(),
        MAX_ALLOWED_CHAINS
    );
}

#[tokio::test]
async fn failed_transaction_logs_do_not_represent_committed_config_changes() {
    let (context, store, program_data, config) = setup().await;
    let mut sequence = 0;
    initialize(&context, store, program_data, config, &mut sequence).await;
    let admin = context.payer.pubkey();
    let (before_bytes, _) = read_config(&context, config).await;
    let (ok, logs) = send(
        &context,
        vec![
            pause_instruction(admin, config, false),
            pause_instruction(admin, config, false),
        ],
        None,
        &mut sequence,
    )
    .await;
    assert!(!ok);
    assert_eq!(events::<ConfigPauseChanged>(&logs).len(), 1);
    assert_eq!(read_config(&context, config).await.0, before_bytes);
}
