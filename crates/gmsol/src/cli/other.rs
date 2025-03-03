use anchor_client::{
    anchor_lang::{system_program, AnchorSerialize},
    solana_sdk::pubkey::Pubkey,
};
use gmsol::utils::instruction::InstructionSerialization;
use solana_sdk::instruction::AccountMeta;

use crate::{GMSOLClient, InstructionBufferCtx};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Initialize the mock chainlink verifier.
    InitMockChainlinkVerifier,
    /// Hex to Base58
    HexToBase58 {
        data: String,
        #[arg(long, short)]
        reverse: bool,
    },
    /// Base64 to Base58
    Base64ToBase58 {
        data: String,
        #[arg(long, short)]
        reverse: bool,
    },
    /// Upgrade Program with the given buffer.
    Upgrade {
        program_id: Pubkey,
        #[arg(long)]
        buffer: Pubkey,
        #[arg(long)]
        authority: Option<Pubkey>,
        #[arg(long)]
        spill: Option<Pubkey>,
    },
    /// Close a program buffer account.
    CloseBuffer {
        address: Pubkey,
        #[arg(long)]
        authority: Option<Pubkey>,
        #[arg(long)]
        spill: Option<Pubkey>,
    },
    /// Set IDL buffer account.
    SetIdlBuffer {
        program_id: Pubkey,
        #[arg(long)]
        buffer: Pubkey,
        #[arg(long)]
        keep_buffer: bool,
    },
    /// Close IDL account.
    CloseIdl {
        program_id: Pubkey,
        account: Option<Pubkey>,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        instruction_buffer: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::InitMockChainlinkVerifier => {
                use mock_chainlink_verifier::{
                    accounts, instruction, DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS,
                    DEFAULT_VERIFIER_ACCOUNT_SEEDS, ID,
                };

                let chainlink_verifier =
                    Pubkey::find_program_address(&[DEFAULT_VERIFIER_ACCOUNT_SEEDS], &ID).0;
                let access_controller =
                    Pubkey::find_program_address(&[DEFAULT_ACCESS_CONTROLLER_ACCOUNT_SEEDS], &ID).0;

                let rpc = client
                    .store_transaction()
                    .program(ID)
                    .anchor_accounts(accounts::Initialize {
                        payer: client.payer(),
                        verifier_account: chainlink_verifier,
                        access_controller,
                        system_program: system_program::ID,
                    })
                    .anchor_args(instruction::Initialize { user: *store });

                crate::utils::send_or_serialize_transaction(
                    store,
                    rpc,
                    instruction_buffer,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::HexToBase58 { data, reverse } => {
                let data = if *reverse {
                    let data = bs58::decode(data)
                        .into_vec()
                        .map_err(gmsol::Error::invalid_argument)?;
                    hex::encode(data)
                } else {
                    let data = hex::decode(data.strip_prefix("0x").unwrap_or(data))
                        .map_err(gmsol::Error::invalid_argument)?;
                    bs58::encode(&data).into_string()
                };
                println!("{data}");
                Ok(())
            }
            Command::Base64ToBase58 { data, reverse } => {
                use base64::prelude::{Engine, BASE64_STANDARD};

                let data = if *reverse {
                    let data = bs58::decode(data)
                        .into_vec()
                        .map_err(gmsol::Error::invalid_argument)?;
                    BASE64_STANDARD.encode(data)
                } else {
                    let data = BASE64_STANDARD
                        .decode(data)
                        .map_err(gmsol::Error::invalid_argument)?;
                    bs58::encode(&data).into_string()
                };

                println!("{data}");
                Ok(())
            }
            Command::Upgrade {
                program_id,
                buffer,
                authority,
                spill,
            } => {
                let rpc = client
                    .store_transaction()
                    .program(system_program::ID)
                    .pre_instruction(solana_sdk::bpf_loader_upgradeable::upgrade(
                        program_id,
                        buffer,
                        &authority.unwrap_or(client.payer()),
                        &spill.unwrap_or(client.payer()),
                    ));

                crate::utils::send_or_serialize_transaction(
                    store,
                    rpc,
                    instruction_buffer,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::CloseBuffer {
                address,
                authority,
                spill,
            } => {
                let rpc = client
                    .store_transaction()
                    .program(system_program::ID)
                    .pre_instruction(solana_sdk::bpf_loader_upgradeable::close(
                        address,
                        &spill.unwrap_or(client.payer()),
                        &authority.unwrap_or(client.payer()),
                    ));

                crate::utils::send_or_serialize_transaction(
                    store,
                    rpc,
                    instruction_buffer,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::SetIdlBuffer {
                program_id,
                buffer,
                keep_buffer: keep_previous_buffer,
            } => {
                use anchor_client::anchor_lang::idl::{IdlAccount, IdlInstruction, IDL_IX_TAG};

                let idl_address = IdlAccount::address(program_id);
                let mut tx = client
                    .store_transaction()
                    .program(*program_id)
                    .accounts(vec![
                        AccountMeta::new(*buffer, false),
                        AccountMeta::new(idl_address, false),
                        AccountMeta::new(client.payer(), true),
                    ])
                    .args({
                        let mut data = IDL_IX_TAG.to_le_bytes().to_vec();
                        data.append(&mut IdlInstruction::SetBuffer.try_to_vec()?);
                        data
                    });

                if !*keep_previous_buffer {
                    tx = tx.merge(
                        client
                            .store_transaction()
                            .program(*program_id)
                            .accounts(vec![
                                AccountMeta::new(*buffer, false),
                                AccountMeta::new(client.payer(), true),
                                AccountMeta::new(client.payer(), false),
                            ])
                            .args({
                                let mut data = IDL_IX_TAG.to_le_bytes().to_vec();
                                data.append(&mut IdlInstruction::Close.try_to_vec()?);
                                data
                            }),
                    );
                }

                crate::utils::send_or_serialize_transaction(
                    store,
                    tx,
                    instruction_buffer,
                    serialize_only,
                    true,
                    |signature| {
                        tracing::info!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::CloseIdl {
                program_id,
                account,
            } => {
                use anchor_client::anchor_lang::idl::{IdlAccount, IdlInstruction, IDL_IX_TAG};

                let idl_address = account.unwrap_or_else(|| IdlAccount::address(program_id));
                let tx = client
                    .store_transaction()
                    .program(*program_id)
                    .accounts(vec![
                        AccountMeta::new(idl_address, false),
                        AccountMeta::new(client.payer(), true),
                        AccountMeta::new(client.payer(), false),
                    ])
                    .args({
                        let mut data = IDL_IX_TAG.to_le_bytes().to_vec();
                        data.append(&mut IdlInstruction::Close.try_to_vec()?);
                        data
                    });

                crate::utils::send_or_serialize_transaction(
                    store,
                    tx,
                    instruction_buffer,
                    serialize_only,
                    true,
                    |signature| {
                        tracing::info!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
        }
    }
}
