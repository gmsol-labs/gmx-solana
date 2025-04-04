use std::num::NonZeroUsize;

use anchor_client::{anchor_lang::system_program, solana_sdk::pubkey::Pubkey};
use gmsol::{idl::IdlOps, utils::instruction::InstructionSerialization};
use gmsol_solana_utils::bundle_builder::BundleOptions;

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
    /// Set the authority of the IDL account.
    SetIdlAuthority {
        program_id: Pubkey,
        #[arg(long)]
        account: Option<Pubkey>,
        #[arg(long, short)]
        new_authority: Pubkey,
    },
    /// Close IDL account.
    CloseIdl {
        program_id: Pubkey,
        #[arg(long)]
        account: Option<Pubkey>,
    },
    /// Resize IDL account.
    ResizeIdl {
        program_id: Pubkey,
        #[arg(long)]
        new_len: u64,
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        force_one_tx: bool,
        /// Buffer to set after resizing.
        #[arg(long, requires = "clear")]
        set_buffer: Option<Pubkey>,
        /// Whether to keep the buffer after it is set
        #[arg(long)]
        keep_buffer: bool,
        /// Maximum number of resizes allowed in a single transaction.
        #[arg(long, default_value_t = NonZeroUsize::new(6).unwrap())]
        resize_limit: NonZeroUsize,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        instruction_buffer: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        priority_lamports: u64,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::InitMockChainlinkVerifier => {
                use gmsol_mock_chainlink_verifier::{
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
                    skip_preflight,
                    Some(priority_lamports),
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
                    skip_preflight,
                    Some(priority_lamports),
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
                    skip_preflight,
                    Some(priority_lamports),
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
                keep_buffer,
            } => {
                let mut tx = client.set_idl_buffer(program_id, buffer)?;

                if !*keep_buffer {
                    tx = tx.merge(client.close_idl_account(program_id, Some(buffer), None)?);
                }

                crate::utils::send_or_serialize_transaction(
                    store,
                    tx,
                    instruction_buffer,
                    serialize_only,
                    skip_preflight,
                    Some(priority_lamports),
                    |signature| {
                        tracing::info!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::SetIdlAuthority {
                program_id,
                account,
                new_authority,
            } => {
                let tx = client.set_idl_authority(program_id, account.as_ref(), new_authority)?;

                crate::utils::send_or_serialize_transaction(
                    store,
                    tx,
                    instruction_buffer,
                    serialize_only,
                    skip_preflight,
                    Some(priority_lamports),
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
                let tx = client.close_idl_account(program_id, account.as_ref(), None)?;

                crate::utils::send_or_serialize_transaction(
                    store,
                    tx,
                    instruction_buffer,
                    serialize_only,
                    skip_preflight,
                    Some(priority_lamports),
                    |signature| {
                        tracing::info!("{signature}");
                        Ok(())
                    },
                )
                .await
            }
            Command::ResizeIdl {
                program_id,
                new_len,
                clear,
                force_one_tx,
                set_buffer,
                keep_buffer,
                resize_limit,
            } => {
                use anchor_client::anchor_lang::idl::IdlAccount;

                const GROWTH_STEP: u64 = 10_000;

                let (data_len, num_additional_instructions) = if *clear {
                    (*new_len, *new_len / GROWTH_STEP)
                } else {
                    let idl_address = IdlAccount::address(program_id);
                    let account = client
                        .account::<IdlAccount>(&idl_address)
                        .await?
                        .ok_or_else(|| gmsol::Error::NotFound)?;
                    let additional_len = (*new_len).saturating_sub(account.data_len.into());

                    if additional_len == 0 {
                        return Err(gmsol::Error::invalid_argument(format!(
                            "the new_len = {new_len} is not greater than the current length = {}",
                            account.data_len
                        )));
                    }

                    (*new_len, additional_len / GROWTH_STEP)
                };

                let options = BundleOptions {
                    force_one_transaction: *force_one_tx,
                    max_packet_size: max_transaction_size,
                    ..Default::default()
                };
                let mut bundle = if *clear {
                    client
                        .close_idl_account(program_id, None, None)?
                        .merge(client.create_idl_account(program_id, data_len)?)
                } else {
                    client.resize_idl_account(program_id, None, data_len)?
                }
                .into_bundle_with_options(options)?;

                let limit = resize_limit.get();

                for count in 0..num_additional_instructions {
                    bundle.try_push_with_opts(
                        client.resize_idl_account(program_id, None, data_len)?,
                        (count as usize + 1) % limit == 0,
                    )?;
                }

                if let Some(buffer) = set_buffer {
                    bundle.push(client.set_idl_buffer(program_id, buffer)?)?;

                    if !*keep_buffer {
                        bundle.push(client.close_idl_account(program_id, Some(buffer), None)?)?;
                    }
                }

                crate::utils::send_or_serialize_bundle_with_default_callback(
                    store,
                    bundle,
                    instruction_buffer,
                    serialize_only,
                    skip_preflight,
                    Some(priority_lamports),
                )
                .await
            }
        }
    }
}
