pub mod svm_helper;

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use anchor_client::{Client, Cluster};
use anchor_lang::declare_program;
use litesvm::LiteSVM;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    message::{VersionedMessage, v0::Message},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Keypair,
    signer::{EncodableKey, Signer},
    system_program,
    sysvar::{clock::Clock, last_restart_slot::LastRestartSlot},
    transaction::VersionedTransaction,
};
use spl_associated_token_account::get_associated_token_address;

use svm_helper::*;

declare_program!(gmsol_store);
declare_program!(gmsol_treasury);

use gmsol_store::{
    ID as GMSOL_STORE_PROGRAM_ID,
    accounts::{Market, Store},
    client::{accounts, args},
    program::GmsolStore,
    types::{CreateOrderParams, OrderKind},
};

pub(crate) fn generate_nonce() -> [u8; 32] {
    use rand::{Rng, distributions::Standard};

    rand::thread_rng()
        .sample_iter(Standard)
        .take(32)
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap()
}
//constants
const EVENT_AUTH: Pubkey = pubkey!("8a4wJ2bMiH6XWDZ7biTnejkss8VG7GMwd9Mg6F5fDfHF");
const STORE: Pubkey = pubkey!("CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN");
const TOKEN: Pubkey = pubkey!("7RNev94wKusSmFEqo3gvv1d1zMweP6sa2LtjJjroAvym"); //"BTC/USD[WBTC-USDC]"
#[test]
fn basic_swap() -> anyhow::Result<()> {
    // --- Initialize LiteSVM and RPC client ---
    let mut litesvm = LiteSVM::new();
    let rpc = RpcClient::new("https://api.mainnet-beta.solana.com");

    // Set sysvar (required by Solana runtime)
    litesvm.set_sysvar::<LastRestartSlot>(&LastRestartSlot {
        last_restart_slot: 246_464_040,
    });

    // Load the on-chain GmSOL Store program
    litesvm
        .add_program_from_file(&GMSOL_STORE_PROGRAM_ID, "./artifacts/store.so")
        .unwrap();

    // --- Create test users and airdrop SOL for fees ---
    let user = Keypair::new();
    let keeper = Keypair::new();
    litesvm.airdrop(&user.pubkey(), 1_000_000_000);
    litesvm.airdrop(&keeper.pubkey(), 1_000_000_000);

    // --- Token setup ---
    let token_in = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"); // USDC
    let token_out = Pubkey::from_str_const("3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh"); // WBTC

    // Create user token accounts
    let token_in_ata = get_associated_token_address(&user.pubkey(), &token_in);
    let token_out_ata = get_associated_token_address(&user.pubkey(), &token_out);

    // Initialize dummy token accounts in LiteSVM
    litesvm.set_account(
        token_in_ata,
        get_dummy_token_account(&litesvm, user.pubkey(), token_in, spl_token::id())?,
    );
    litesvm.set_account(
        token_out_ata,
        get_dummy_token_account(&litesvm, user.pubkey(), token_out, spl_token::id())?,
    );

    // --- Initialize Anchor client ---
    let program = Client::new(Cluster::Mainnet, Rc::new(user.insecure_clone()))
        .program(GMSOL_STORE_PROGRAM_ID)
        .unwrap();

    // === Derive PDAs ===
    let store = STORE; // assuming imported from svm_helper
    let market = Pubkey::find_program_address(
        &[b"market", store.as_ref(), TOKEN.as_ref()],
        &GMSOL_STORE_PROGRAM_ID,
    )
    .0;

    let nonce = generate_nonce();
    let order = Pubkey::find_program_address(
        &[
            b"order",
            store.as_ref(),
            user.pubkey().as_ref(),
            nonce.as_ref(),
        ],
        &GMSOL_STORE_PROGRAM_ID,
    )
    .0;

    let user_pda = Pubkey::find_program_address(
        &[b"user", store.as_ref(), user.pubkey().as_ref()],
        &GMSOL_STORE_PROGRAM_ID,
    )
    .0;

    let swap_in_amount = 100_000;

    // Define order parameters
    let params = CreateOrderParams {
        is_collateral_long: false,
        kind: OrderKind::MarketSwap,
        swap_path_length: 1,
        size_delta_value: 0,
        initial_collateral_delta_amount: swap_in_amount,
        is_long: true,
        execution_lamports: 300_000,
        should_unwrap_native_token: true,
        valid_from_ts: None,
        acceptable_price: None,
        trigger_price: None,
        min_output: None,
        decrease_position_swap_type: None,
    };

    // --- Build instructions ---
    let prepare_user_ix = program
        .request()
        .accounts(accounts::PrepareUser {
            owner: user.pubkey(),
            store,
            user: user_pda,
            system_program: system_program::id(),
        })
        .args(args::PrepareUser {})
        .instructions()?
        .remove(0);

    let initial_collateral_token_escrow = get_associated_token_address(&order, &token_in);
    let final_output_token_escrow = get_associated_token_address(&order, &token_out);
    let oracle = Pubkey::from_str_const("AywftYs9BX3GmzQ5RPaaLxnJZGvMpW91RfT5rU5stvUG");

    // Set token escrow accounts in SVM
    litesvm.set_account(
        initial_collateral_token_escrow,
        get_dummy_token_account(&litesvm, order, token_in, spl_token::id())?,
    );
    litesvm.set_account(
        final_output_token_escrow,
        get_dummy_token_account(&litesvm, order, token_out, spl_token::id())?,
    );

    // Build CreateOrderV2 instruction
    let create_order_v2_ix = program
        .request()
        .accounts(accounts::CreateOrderV2 {
            owner: user.pubkey(),
            receiver: user.pubkey(),
            store,
            market,
            user: user_pda,
            system_program: system_program::id(),
            order,
            initial_collateral_token: Some(token_in),
            final_output_token: token_out,
            initial_collateral_token_source: Some(token_in_ata),
            initial_collateral_token_escrow: Some(initial_collateral_token_escrow),
            final_output_token_escrow: Some(final_output_token_escrow),
            callback_program: Some(GMSOL_STORE_PROGRAM_ID),
            callback_authority: Some(GMSOL_STORE_PROGRAM_ID),
            callback_partitioned_data_account: Some(GMSOL_STORE_PROGRAM_ID),
            callback_shared_data_account: Some(GMSOL_STORE_PROGRAM_ID),
            event_authority: EVENT_AUTH,
            associated_token_program: pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
            program: GMSOL_STORE_PROGRAM_ID,
            token_program: spl_token::id(),
            long_token_escrow: None,
            short_token_escrow: None,
            long_token: None,
            short_token: None,
            position: None,
        })
        .args(args::CreateOrderV2 {
            nonce,
            params,
            callback_version: None,
        })
        .accounts(AccountMeta {
            pubkey: market,
            is_signer: false,
            is_writable: false,
        })
        .instructions()?
        .remove(0);

    // --- Hydrate SVM with necessary mainnet accounts ---
    let token_map = Pubkey::from_str_const("7cEiyyEB2VGMtnKLb7Ro7SsJKYZVBZjpS8ihBD8jR938");
    hydrate_svm(
        &mut litesvm,
        fetch_mainnet_accounts(
            vec![
                store, token_in, token_out, market, EVENT_AUTH, token_map, oracle,
            ],
            &rpc,
        ),
    );

    // --- Execute Transaction ---
    execute_transaction(
        &mut litesvm,
        vec![prepare_user_ix, create_order_v2_ix],
        vec![user.insecure_clone()],
        user.insecure_clone(),
    );
    // === Timestamp for Execution ===
    let recent_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // === Derive Market Vault PDAs ===
    let initial_collateral_token_vault = Pubkey::find_program_address(
        &[b"market_vault", STORE.as_ref(), token_in.as_ref()],
        &GMSOL_STORE_PROGRAM_ID,
    )
    .0;

    let final_output_token_vault = Pubkey::find_program_address(
        &[b"market_vault", STORE.as_ref(), token_out.as_ref()],
        &GMSOL_STORE_PROGRAM_ID,
    )
    .0;

    // === Set Vault Accounts in LiteSVM ===
    litesvm.set_account(
        initial_collateral_token_vault,
        get_dummy_token_account(&litesvm, STORE, token_in, spl_token::id())?,
    );
    litesvm.set_account(
        final_output_token_vault,
        get_dummy_token_account(&litesvm, STORE, token_out, spl_token::id())?,
    );

    // === Build ExecuteIncreaseOrSwapOrderV2 Instruction ===
    let execute_increase_or_swap_order_v2_ix = program
        .request()
        .accounts(accounts::ExecuteIncreaseOrSwapOrderV2 {
            authority: user.pubkey(),
            owner: user.pubkey(),
            store: STORE,
            token_map,
            market,
            oracle,
            user: user_pda,
            system_program: system_program::id(),
            order,
            // Token inputs/outputs
            initial_collateral_token: Some(token_in),
            final_output_token: Some(token_out),
            initial_collateral_token_escrow: Some(initial_collateral_token_escrow),
            final_output_token_escrow: Some(final_output_token_escrow),
            initial_collateral_token_vault: Some(initial_collateral_token_vault),
            final_output_token_vault: Some(final_output_token_vault),
            // Callbacks
            callback_program: Some(GMSOL_STORE_PROGRAM_ID),
            callback_authority: Some(GMSOL_STORE_PROGRAM_ID),
            callback_partitioned_data_account: Some(GMSOL_STORE_PROGRAM_ID),
            callback_shared_data_account: Some(GMSOL_STORE_PROGRAM_ID),
            // Misc
            event_authority: EVENT_AUTH,
            program: GMSOL_STORE_PROGRAM_ID,
            token_program: spl_token::id(),
            // Optional unused fields
            event: None,
            long_token_escrow: None,
            short_token_escrow: None,
            long_token: None,
            short_token: None,
            long_token_vault: None,
            short_token_vault: None,
            position: None,
        })
        .args(args::ExecuteIncreaseOrSwapOrderV2 {
            recent_timestamp,
            execution_fee: 300_000,
            throw_on_execution_error: true,
        })
        .instructions()?
        .remove(0);

    /*
    TODO:
      - Grant executor permission to `keeper`
      - Add missing accounts:
          * Market feeds
          * Market token account
          * Virtual inventory accounts
      - Then include this instruction in the transaction sequence:
            execute_transaction(
                &mut litesvm,
                vec![
                    execute_increase_or_swap_order_v2_ix,
                ],
                vec![user.insecure_clone(), keeper.insecure_clone()],
                keeper.insecure_clone(),
            );
    */
    Ok(())
}
