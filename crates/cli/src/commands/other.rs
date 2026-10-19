use std::num::NonZeroUsize;

use gmsol_sdk::{
    ops::IdlOps,
    programs::anchor_lang::{self, prelude::Pubkey, system_program},
    utils::base64::{decode_base64, encode_base64},
};
use solana_loader_v3_interface::instruction as bpf_loader_upgradeable;

/// Miscellaneous useful commands.
#[derive(Debug, clap::Args)]
pub struct Other {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
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
    UpgradeProgram {
        program_id: Pubkey,
        #[arg(long)]
        buffer: Pubkey,
        #[arg(long)]
        authority: Option<Pubkey>,
        #[arg(long)]
        spill: Option<Pubkey>,
    },
    /// Close a program buffer account.
    CloseProgramBuffer {
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

impl super::Command for Other {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let options = ctx.bundle_options();
        let bundle = match &self.command {
            Command::HexToBase58 { data, reverse } => {
                let data = if *reverse {
                    let data = bs58::decode(data).into_vec()?;
                    hex::encode(data)
                } else {
                    let data = hex::decode(data.strip_prefix("0x").unwrap_or(data))?;
                    bs58::encode(&data).into_string()
                };
                println!("{data}");
                return Ok(());
            }
            Command::Base64ToBase58 { data, reverse } => {
                let data = if *reverse {
                    let data = bs58::decode(data).into_vec()?;
                    encode_base64(&data)
                } else {
                    let data = decode_base64(data)?;
                    bs58::encode(&data).into_string()
                };

                println!("{data}");
                return Ok(());
            }
            Command::UpgradeProgram {
                program_id,
                buffer,
                authority,
                spill,
            } => client
                .store_transaction()
                .program(system_program::ID)
                .pre_instruction(
                    bpf_loader_upgradeable::upgrade(
                        program_id,
                        buffer,
                        &authority.unwrap_or(client.payer()),
                        &spill.unwrap_or(client.payer()),
                    ),
                    true,
                )
                .into_bundle_with_options(options)?,
            Command::CloseProgramBuffer {
                address,
                authority,
                spill,
            } => client
                .store_transaction()
                .program(system_program::ID)
                .pre_instruction(
                    bpf_loader_upgradeable::close(
                        address,
                        &spill.unwrap_or(client.payer()),
                        &authority.unwrap_or(client.payer()),
                    ),
                    true,
                )
                .into_bundle_with_options(options)?,
            Command::SetIdlBuffer {
                program_id,
                buffer,
                keep_buffer,
            } => {
                let mut tx = client.set_idl_buffer(program_id, buffer)?;

                if !*keep_buffer {
                    tx = tx.merge(client.close_idl_account(program_id, Some(buffer), None)?);
                }

                tx.into_bundle_with_options(options)?
            }
            Command::SetIdlAuthority {
                program_id,
                account,
                new_authority,
            } => client
                .set_idl_authority(program_id, account.as_ref(), new_authority)?
                .into_bundle_with_options(options)?,
            Command::CloseIdl {
                program_id,
                account,
            } => client
                .close_idl_account(program_id, account.as_ref(), None)?
                .into_bundle_with_options(options)?,
            Command::ResizeIdl {
                program_id,
                new_len,
                clear,
                set_buffer,
                keep_buffer,
                resize_limit,
            } => {
                use anchor_lang::idl::IdlAccount;

                const GROWTH_STEP: u64 = 10_000;

                let (data_len, num_additional_instructions) = if *clear {
                    (*new_len, *new_len / GROWTH_STEP)
                } else {
                    let idl_address = IdlAccount::address(program_id);
                    let account = client
                        .account::<IdlAccount>(&idl_address)
                        .await?
                        .ok_or_else(|| gmsol_sdk::Error::NotFound)?;
                    let additional_len = (*new_len).saturating_sub(account.data_len.into());

                    if additional_len == 0 {
                        eyre::bail!(
                            "the new_len = {new_len} is not greater than the current length = {}",
                            account.data_len
                        );
                    }

                    (*new_len, additional_len / GROWTH_STEP)
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
                    bundle
                        .try_push_with_opts(
                            client.resize_idl_account(program_id, None, data_len)?,
                            (count as usize + 1) % limit == 0,
                        )
                        .map_err(|(_, err)| err)?;
                }

                if let Some(buffer) = set_buffer {
                    bundle.push(client.set_idl_buffer(program_id, buffer)?)?;

                    if !*keep_buffer {
                        bundle.push(client.close_idl_account(program_id, Some(buffer), None)?)?;
                    }
                }

                bundle
            }
        };
        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
