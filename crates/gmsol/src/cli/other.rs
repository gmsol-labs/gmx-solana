use anchor_client::{anchor_lang::system_program, solana_sdk::pubkey::Pubkey};
use gmsol::utils::instruction::InstructionSerialization;

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
    /// Hex to base58
    HexToBase58 { hex: String },
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

                crate::utils::send_or_serialize_rpc(
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
            Command::HexToBase58 { hex } => {
                let data = hex::decode(hex).map_err(gmsol::Error::invalid_argument)?;
                let data = bs58::encode(&data).into_string();
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

                crate::utils::send_or_serialize_rpc(
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

                crate::utils::send_or_serialize_rpc(
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
        }
    }
}
