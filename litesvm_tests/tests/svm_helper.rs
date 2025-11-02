use anyhow::{Error, Result};
use litesvm::LiteSVM;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    account::Account,
    instruction::Instruction,
    message::{VersionedMessage, v0::Message},
    program_pack::Pack,
    pubkey,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use spl_token::state::{Account as TokenAccount, AccountState};
/// Program and constant IDs
const GMSOL_STORE_PROGRAM_ID: Pubkey = pubkey!("Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo");
const STORE: Pubkey = pubkey!("CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN");
const TOKEN: Pubkey = pubkey!("xiLDzynfr7JEoYinAEunZtdz9ubjVAqa5Ap7gJ9y43L"); // SOL/USD[WSOL-WSOL]
const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
const USDC: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
const ATP_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const EVENT_AUTH: Pubkey = pubkey!("8a4wJ2bMiH6XWDZ7biTnejkss8VG7GMwd9Mg6F5fDfHF");

pub fn execute_transaction(
    svm: &mut LiteSVM,
    instructions: Vec<Instruction>,
    signers: Vec<Keypair>,
    payer: Keypair,
) {
    let message = Message::try_compile(&payer.pubkey(), &instructions, &[], svm.latest_blockhash())
        .expect("failed to compile message");

    let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &signers)
        .expect("failed to create transaction");

    let result = svm.send_transaction(tx).expect("transaction failed");
    println!("{:#?}", result.logs);
}

pub fn hydrate_svm(svm: &mut LiteSVM, accounts: Vec<(Pubkey, Account)>) {
    for (addr, acc) in accounts {
        svm.set_account(addr, acc)
            .expect("failed to insert account");
    }
}

pub fn get_dummy_token_account(
    svm: &LiteSVM,
    owner: Pubkey,
    mint: Pubkey,
    token_program: Pubkey,
) -> Result<Account, Error> {
    let token_account = TokenAccount {
        mint,
        owner,
        amount: 10_000_000_000,
        delegate: None.into(),
        state: AccountState::Initialized,
        is_native: None.into(),
        delegated_amount: 0,
        close_authority: None.into(),
    };

    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(token_account, &mut data)?;

    Ok(Account {
        lamports: svm.minimum_balance_for_rent_exemption(TokenAccount::LEN),
        data,
        owner: token_program,
        executable: false,
        rent_epoch: 0,
    })
}

pub fn fetch_mainnet_accounts(addresses: Vec<Pubkey>, rpc: &RpcClient) -> Vec<(Pubkey, Account)> {
    let accounts_result = rpc
        .get_multiple_accounts(&addresses)
        .expect("failed to fetch accounts");

    addresses
        .into_iter()
        .zip(accounts_result)
        .filter_map(|(addr, acc_opt)| acc_opt.map(|acc| (addr, acc)))
        .collect()
}
