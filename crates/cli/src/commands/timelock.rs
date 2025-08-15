use std::{ops::Deref, path::PathBuf, time::Duration};

use gmsol_sdk::{
    core::oracle::PriceProviderKind,
    ops::{AddressLookupTableOps, TimelockOps},
    programs::{
        anchor_lang::prelude::{AccountMeta, Pubkey},
        gmsol_timelock::accounts::{Executor, TimelockConfig},
    },
    solana_utils::solana_sdk::{
        instruction::Instruction,
        message::{MessageHeader, VersionedMessage},
        signature::{read_keypair_file, Keypair},
        signer::Signer,
        transaction::VersionedTransaction,
    },
    utils::{base64::decode_base64, zero_copy::ZeroCopy},
};

use crate::config::DisplayOptions;

/// Timelock commands.
#[derive(Debug, clap::Args)]
pub struct Timelock {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Get current config.
    Config,
    /// Get executor.
    Executor { role: String },
    /// Get executor wallet.
    ExecutorWallet { role: String },
    /// Initialize timelock config.
    InitConfig {
        #[arg(long, default_value = "1d", value_parser = humantime::parse_duration)]
        initial_delay: Duration,
    },
    /// Increase timelock delay.
    IncreaseDelay {
        #[arg(value_parser = humantime::parse_duration)]
        delta: Duration,
    },
    /// Init executor.
    InitExecutor { role: String },
    /// Approve a timelocked instruction.
    Approve {
        buffers: Vec<Pubkey>,
        #[arg(long)]
        role: Option<String>,
    },
    /// Cancel a timelocked instruction.
    Cancel {
        buffers: Vec<Pubkey>,
        #[arg(long)]
        executor: Option<Pubkey>,
        #[arg(long)]
        rent_receiver: Option<Pubkey>,
    },
    /// Execute a timelocked instruction.
    Execute { buffers: Vec<Pubkey> },
    /// Revoke role.
    RevokeRole {
        /// User.
        authority: Pubkey,
        /// Role.
        role: String,
    },
    /// Set expected price provider.
    SetExpectedPriceProvider {
        /// Token.
        token: Pubkey,
        /// New Price Provider.
        provider: PriceProviderKind,
        /// Token map.
        #[arg(long)]
        token_map: Option<Pubkey>,
    },
    /// Create instruction buffer for transaction.
    CreateIxBuffer {
        /// Role for the instruction buffers.
        #[arg(long)]
        role: String,
        /// Keypairs for the instruction buffer accounts.
        #[arg(long, short, group = "buffer-signers")]
        buffers: Vec<PathBuf>,
        #[cfg(feature = "squads")]
        #[arg(long, group = "buffer-signers")]
        use_squads: bool,
        /// Base58-encoded transaction.
        transaction: String,
    },
}

impl super::Command for Timelock {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let options = ctx.bundle_options();
        let store = &ctx.store;
        let bundle = match &self.command {
            Command::Config => {
                let config_address = client.find_timelock_config_address(store);
                let config = client
                    .account::<ZeroCopy<TimelockConfig>>(&config_address)
                    .await?;
                match config {
                    Some(config) => {
                        let config = config.0;
                        let out = ctx.config().output().display_many(
                            [serde_json::json!({
                                "address": config_address,
                                "delay_seconds": config.delay,
                            })],
                            DisplayOptions::table_projection([
                                ("address", "Address"),
                                ("delay_seconds", "Delay (s)"),
                            ]),
                        )?;
                        println!("{out}");
                    }
                    None => {
                        let out = ctx
                            .config()
                            .output()
                            .display_value_with_label("status", "Not initialized")?;
                        println!("{out}");
                    }
                }
                return Ok(());
            }
            Command::Executor { role } => {
                let executor = get_and_validate_executor_address(client, store, role).await?;
                let wallet = client.find_executor_wallet_address(&executor);
                let out = ctx.config().output().display_many(
                    [serde_json::json!({
                        "executor": executor,
                        "wallet": wallet,
                    })],
                    DisplayOptions::table_projection([
                        ("executor", "Executor"),
                        ("wallet", "Wallet"),
                    ]),
                )?;
                println!("{out}");
                return Ok(());
            }
            Command::ExecutorWallet { role } => {
                let executor = get_and_validate_executor_address(client, store, role).await?;
                let wallet = client.find_executor_wallet_address(&executor);
                let out = ctx
                    .config()
                    .output()
                    .display_value_with_label("wallet", wallet)?;
                println!("{out}");
                return Ok(());
            }
            Command::InitConfig { initial_delay } => {
                let (rpc, config) = client
                    .initialize_timelock_config(store, initial_delay.as_secs().try_into()?)
                    .swap_output(());
                let out = ctx
                    .config()
                    .output()
                    .display_value_with_label("config", config)?;
                println!("{out}");
                rpc.into_bundle_with_options(options)?
            }
            Command::IncreaseDelay { delta } => client
                .increase_timelock_delay(store, delta.as_secs().try_into()?)
                .into_bundle_with_options(options)?,
            Command::InitExecutor { role } => {
                let (rpc, executor) = client.initialize_executor(store, role)?.swap_output(());
                let out = ctx
                    .config()
                    .output()
                    .display_value_with_label("executor", executor)?;
                println!("{out}");
                rpc.into_bundle_with_options(options)?
            }
            Command::Approve { buffers, role } => client
                .approve_timelocked_instructions(
                    store,
                    buffers.iter().copied(),
                    role.as_ref().map(|s| s.as_str()),
                )
                .await?
                .into_bundle_with_options(options)?,
            Command::Cancel {
                buffers,
                executor,
                rent_receiver,
            } => client
                .cancel_timelocked_instructions(
                    store,
                    buffers.iter().copied(),
                    executor.as_ref(),
                    rent_receiver.as_ref(),
                )
                .await?
                .into_bundle_with_options(options)?,
            Command::Execute { buffers } => {
                let mut txns = client.bundle_with_options(options);
                for buffer in buffers {
                    let rpc = client
                        .execute_timelocked_instruction(store, buffer, None)
                        .await?;
                    txns.push(rpc)?;
                }
                txns
            }
            Command::RevokeRole { authority, role } => client
                .timelock_bypassed_revoke_role(store, role, authority)
                .into_bundle_with_options(options)?,
            Command::SetExpectedPriceProvider {
                token,
                provider,
                token_map,
            } => {
                let token_map = match *token_map {
                    Some(token_map) => token_map,
                    None => client
                        .authorized_token_map_address(store)
                        .await?
                        .ok_or(gmsol_sdk::Error::NotFound)?,
                };
                client
                    .timelock_bypassed_set_epxected_price_provider(
                        store, &token_map, token, *provider,
                    )
                    .into_bundle_with_options(options)?
            }
            Command::CreateIxBuffer {
                role,
                buffers,
                #[cfg(feature = "squads")]
                use_squads,
                transaction,
            } => {
                let transaction = decode_base64(transaction)?;
                let message = if let Ok(message) =
                    bincode::deserialize::<VersionedMessage>(&transaction)
                {
                    message
                } else if let Ok(txn) = bincode::deserialize::<VersionedTransaction>(&transaction) {
                    txn.message
                } else {
                    eyre::bail!("failed to deserialize the message");
                };
                let ixs = decode_message(client, &message).await?;

                #[cfg(feature = "squads")]
                {
                    use eyre::OptionExt;
                    use gmsol_sdk::{
                        client::squads::{SquadsOps, VaultTransactionOptions},
                        solana_utils::{
                            solana_sdk::signer::null_signer::NullSigner, utils::inspect_transaction,
                        },
                    };

                    if *use_squads {
                        let len = ixs.len();
                        let steps = len + 1;
                        let (multisig, vault_index) =
                            client.squads_ctx().ok_or_eyre("must use with `--squads`")?;
                        let host_client = client.host_client();
                        let mut bundle = host_client.bundle_with_options(options);
                        for (idx, ix) in ixs.into_iter().enumerate() {
                            let mut message = Default::default();
                            let (rpc, transaction) = host_client
                                .squads_create_vault_transaction(
                                    &multisig,
                                    vault_index,
                                    |ephemeral_signers| {
                                        let buffer = ephemeral_signers[0];
                                        let rpc = client
                                            .create_timelocked_instruction(
                                                store,
                                                role,
                                                NullSigner::new(&buffer),
                                                ix,
                                            )?
                                            .swap_output(())
                                            .0;
                                        let out = ctx
                                            .config()
                                            .output()
                                            .display_many(
                                                [serde_json::json!({
                                                    "index": idx,
                                                    "buffer": buffer,
                                                })],
                                                DisplayOptions::table_projection([
                                                    ("index", "Index"),
                                                    ("buffer", "Buffer"),
                                                ]),
                                            )
                                            .map_err(gmsol_sdk::Error::custom)?;
                                        println!("{out}");
                                        message = rpc.message_with_blockhash_and_options(
                                            Default::default(),
                                            true,
                                            None,
                                            None,
                                        )?;
                                        Ok(message.clone())
                                    },
                                    VaultTransactionOptions {
                                        ephemeral_signers: 1,
                                        ..Default::default()
                                    },
                                    Some(idx as u64),
                                )
                                .await?
                                .swap_output(());
                            println!("Adding a vault transaction {idx}: id = {transaction}");
                            println!(
                                "Inspector URL for transaction {idx}: {}",
                                inspect_transaction(&message, Some(client.cluster()), false),
                            );

                            let txn_count = idx + 1;
                            let confirmation = dialoguer::Confirm::new()
                                .with_prompt(format!(
                            "[{txn_count}/{steps}] Confirm to add vault transaction {idx} ?"
                        ))
                                .default(false)
                                .interact()
                                .map_err(gmsol_sdk::Error::custom)?;

                            if !confirmation {
                                tracing::info!("Cancelled");
                                return Ok(());
                            }

                            bundle.push(rpc)?;
                        }
                        let confirmation = dialoguer::Confirm::new()
                            .with_prompt(format!(
                                "[{steps}/{steps}] Confirm creation of {len} vault transactions?"
                            ))
                            .default(false)
                            .interact()
                            .map_err(gmsol_sdk::Error::custom)?;

                        if !confirmation {
                            tracing::info!("Cancelled");
                            return Ok(());
                        }
                        client.send_bundle(bundle).await?;
                        return Ok(());
                    }
                }

                ctx.require_not_ix_buffer_mode()?;

                let mut bundle = client.bundle_with_options(options);
                let mut buffers = buffers.iter();
                for (idx, ix) in ixs.into_iter().enumerate() {
                    let buffer = match buffers.next() {
                        Some(buffer) => {
                            read_keypair_file(buffer).map_err(gmsol_sdk::Error::custom)?
                        }
                        None => Keypair::new(),
                    };
                    let (rpc, buffer) = client
                        .create_timelocked_instruction(store, role, buffer, ix)?
                        .swap_output(());
                    let out = ctx
                        .config()
                        .output()
                        .display_many(
                            [serde_json::json!({
                                "index": idx,
                                "buffer": buffer,
                            })],
                            DisplayOptions::table_projection([
                                ("index", "Index"),
                                ("buffer", "Buffer"),
                            ]),
                        )
                        .map_err(gmsol_sdk::Error::custom)?;
                    println!("{out}");
                    bundle.push(rpc)?;
                }
                bundle
            }
        };
        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

async fn get_and_validate_executor_address<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    store: &Pubkey,
    role: &str,
) -> gmsol_sdk::Result<Pubkey> {
    let executor = client.find_executor_address(store, role)?;
    let account = client
        .account::<ZeroCopy<Executor>>(&executor)
        .await?
        .ok_or(gmsol_sdk::Error::NotFound)?;
    if account.0.role_name()? == role {
        Ok(executor)
    } else {
        Err(gmsol_sdk::Error::custom(format!(
            "invalid executor account found: {executor}"
        )))
    }
}

async fn decode_message<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    message: &VersionedMessage,
) -> gmsol_sdk::Result<Vec<Instruction>> {
    let (metas, ixs) =
        match message {
            VersionedMessage::Legacy(m) => {
                let metas = decode_static_keys(&m.header, &m.account_keys)?.to_metas();

                (metas, &m.instructions)
            }
            VersionedMessage::V0(m) => {
                let mut metas = decode_static_keys(&m.header, &m.account_keys)?.to_metas();

                let mut writable = Vec::default();
                let mut readonly = Vec::default();
                for alt in &m.address_table_lookups {
                    let lut = client
                        .alt(&alt.account_key)
                        .await?
                        .ok_or(gmsol_sdk::Error::NotFound)?;
                    for idx in alt.writable_indexes.iter() {
                        let pubkey = lut.addresses.get(usize::from(*idx)).ok_or_else(|| {
                            gmsol_sdk::Error::custom("invalid transaction messsage")
                        })?;
                        writable.push(AccountMeta::new(*pubkey, false));
                    }
                    for idx in alt.readonly_indexes.iter() {
                        let pubkey = lut.addresses.get(usize::from(*idx)).ok_or_else(|| {
                            gmsol_sdk::Error::custom("invalid transaction messsage")
                        })?;
                        readonly.push(AccountMeta::new_readonly(*pubkey, false));
                    }
                }

                metas.append(&mut writable);
                metas.append(&mut readonly);

                (metas, &m.instructions)
            }
        };

    ixs.iter()
        .map(|ix| {
            let program_id = metas
                .get(usize::from(ix.program_id_index))
                .ok_or_else(|| gmsol_sdk::Error::custom("invalid transaction message"))?;
            let accounts = ix
                .accounts
                .iter()
                .map(|idx| {
                    metas
                        .get(usize::from(*idx))
                        .ok_or_else(|| gmsol_sdk::Error::custom("invalid transaction message"))
                        .cloned()
                })
                .collect::<gmsol_sdk::Result<Vec<AccountMeta>>>()?;
            Ok(Instruction::new_with_bytes(
                program_id.pubkey,
                &ix.data,
                accounts,
            ))
        })
        .collect()
}

struct StaticKeys<'a> {
    writable_signed: &'a [Pubkey],
    readonly_signed: &'a [Pubkey],
    writable_unsigned: &'a [Pubkey],
    readonly_unsigned: &'a [Pubkey],
}

impl StaticKeys<'_> {
    fn to_metas(&self) -> Vec<AccountMeta> {
        let Self {
            writable_signed,
            readonly_signed,
            writable_unsigned,
            readonly_unsigned,
        } = self;
        writable_signed
            .iter()
            .map(|p| AccountMeta::new(*p, true))
            .chain(
                readonly_signed
                    .iter()
                    .map(|p| AccountMeta::new_readonly(*p, true)),
            )
            .chain(
                writable_unsigned
                    .iter()
                    .map(|p| AccountMeta::new(*p, false)),
            )
            .chain(
                readonly_unsigned
                    .iter()
                    .map(|p| AccountMeta::new_readonly(*p, false)),
            )
            .collect::<Vec<AccountMeta>>()
    }
}

fn decode_static_keys<'a>(
    header: &MessageHeader,
    account_keys: &'a [Pubkey],
) -> gmsol_sdk::Result<StaticKeys<'a>> {
    let end = account_keys.len();
    let num_signed: usize = header.num_required_signatures.into();
    let num_readonly_signed: usize = header.num_readonly_signed_accounts.into();
    let num_readonly_unsigned: usize = header.num_readonly_unsigned_accounts.into();

    if end < num_signed + num_readonly_unsigned || num_signed < num_readonly_signed {
        return Err(gmsol_sdk::Error::custom("invalid transaction message"));
    }

    let writable_signed_end = num_signed - num_readonly_signed;
    let readonly_signed_end = writable_signed_end + num_readonly_signed;
    let writable_unsigned_end = end - num_readonly_unsigned;

    Ok(StaticKeys {
        writable_signed: &account_keys[0..writable_signed_end],
        readonly_signed: &account_keys[writable_signed_end..readonly_signed_end],
        writable_unsigned: &account_keys[readonly_signed_end..writable_unsigned_end],
        readonly_unsigned: &account_keys[writable_unsigned_end..end],
    })
}
