use std::{collections::BTreeSet, ops::Deref, path::Path, sync::Arc};

use admin::Admin;
use alt::Alt;
use competition::Competition;
use configuration::Configuration;
use either::Either;
use enum_dispatch::enum_dispatch;
use exchange::Exchange;
use eyre::OptionExt;
use get_pubkey::GetPubkey;
use glv::Glv;
use gmsol_sdk::{
    ops::{AddressLookupTableOps, TimelockOps},
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        bundle_builder::{Bundle, BundleBuilder, BundleOptions, SendBundleOptions},
        instruction_group::{ComputeBudgetOptions, GetInstructionsOptions},
        signer::LocalSignerRef,
        solana_client::rpc_config::RpcSendTransactionConfig,
        solana_sdk::{
            signature::{Keypair, NullSigner, Signature},
            transaction::VersionedTransaction,
        },
        transaction_builder::default_before_sign,
        utils::{inspect_transaction, WithSlot},
    },
    utils::instruction_serialization::{serialize_message, InstructionSerialization},
    Client,
};
use gt::Gt;
use init_config::InitConfig;

use inspect::Inspect;
use market::Market;
use other::Other;
#[cfg(feature = "remote-wallet")]
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use timelock::Timelock;
use treasury::Treasury;
use user::User;

use crate::config::{Config, InstructionBuffer, Payer};
use crate::config::{DisplayOptions, OutputFormat};

mod admin;
mod alt;
mod competition;
mod configuration;
mod exchange;
mod get_pubkey;
mod glv;
mod gt;
mod init_config;
mod inspect;
mod market;
mod other;
mod timelock;
mod treasury;
mod user;

/// Utils for command implementations.
pub mod utils;

/// Commands.
#[enum_dispatch(Command)]
#[derive(Debug, clap::Subcommand)]
pub enum Commands {
    /// Initialize config file.
    InitConfig(InitConfig),
    /// Get pubkey of the payer.
    Pubkey(GetPubkey),
    /// Exchange-related commands.
    Exchange(Box<Exchange>),
    /// User account commands.
    User(User),
    /// GT-related commands.
    Gt(Gt),
    /// Address Lookup Table commands.
    Alt(Alt),
    /// Administrative commands.
    Admin(Admin),
    /// Timelock commands.
    Timelock(Timelock),
    /// Treasury management commands.
    Treasury(Treasury),
    /// Market management commands.
    Market(Market),
    /// GLV management commands.
    Glv(Glv),
    /// On-chain configuration and features management.
    Configuration(Configuration),
    /// Competition management commands.
    Competition(Competition),
    /// Inspect protocol data.
    Inspect(Inspect),
    /// Miscellaneous useful commands.
    Other(Other),
}

#[enum_dispatch]
pub(crate) trait Command {
    fn is_client_required(&self) -> bool {
        false
    }

    async fn execute(&self, ctx: Context<'_>) -> eyre::Result<()>;
}

impl<T: Command> Command for Box<T> {
    fn is_client_required(&self) -> bool {
        (**self).is_client_required()
    }

    async fn execute(&self, ctx: Context<'_>) -> eyre::Result<()> {
        (**self).execute(ctx).await
    }
}

pub(crate) struct Context<'a> {
    store: Pubkey,
    config_path: &'a Path,
    config: &'a Config,
    client: Option<&'a CommandClient>,
    _verbose: bool,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        store: Pubkey,
        config_path: &'a Path,
        config: &'a Config,
        client: Option<&'a CommandClient>,
        verbose: bool,
    ) -> Self {
        Self {
            store,
            config_path,
            config,
            client,
            _verbose: verbose,
        }
    }

    pub(crate) fn config(&self) -> &Config {
        self.config
    }

    pub(crate) fn client(&self) -> eyre::Result<&CommandClient> {
        self.client.ok_or_eyre("client is not provided")
    }

    pub(crate) fn store(&self) -> &Pubkey {
        &self.store
    }

    pub(crate) fn bundle_options(&self) -> BundleOptions {
        self.config.bundle_options()
    }

    pub(crate) fn require_not_serialize_only_mode(&self) -> eyre::Result<()> {
        let client = self.client()?;
        if client.serialize_only.is_some() {
            eyre::bail!("serialize-only mode is not supported");
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_not_ix_buffer_mode(&self) -> eyre::Result<()> {
        let client = self.client()?;
        if client.ix_buffer_ctx.is_some() {
            eyre::bail!("instruction buffer is not supported");
        } else {
            Ok(())
        }
    }

    pub(crate) fn _verbose(&self) -> bool {
        self._verbose
    }
}

struct IxBufferCtx<C> {
    buffer: InstructionBuffer,
    client: Client<C>,
    is_draft: bool,
}

pub(crate) struct CommandClient {
    store: Pubkey,
    client: Client<LocalSignerRef>,
    ix_buffer_ctx: Option<IxBufferCtx<LocalSignerRef>>,
    serialize_only: Option<InstructionSerialization>,
    verbose: bool,
    priority_lamports: u64,
    skip_preflight: bool,
    luts: BTreeSet<Pubkey>,
    output_format: OutputFormat,
}

impl CommandClient {
    pub(crate) fn new(
        config: &Config,
        #[cfg(feature = "remote-wallet")] wallet_manager: &mut Option<
            std::rc::Rc<RemoteWalletManager>,
        >,
        verbose: bool,
    ) -> eyre::Result<Self> {
        let Payer { payer, proposer } = config.create_wallet(
            #[cfg(feature = "remote-wallet")]
            Some(wallet_manager),
        )?;

        let cluster = config.cluster();
        let options = config.options();
        let client = Client::new_with_options(cluster.clone(), payer, options.clone())?;
        let ix_buffer_client = proposer
            .map(|payer| Client::new_with_options(cluster.clone(), payer, options))
            .transpose()?;
        let ix_buffer = config.ix_buffer()?;

        Ok(Self {
            store: config.store_address(),
            client,
            ix_buffer_ctx: ix_buffer_client.map(|client| {
                let buffer = ix_buffer.expect("must be present");
                IxBufferCtx {
                    buffer,
                    client,
                    is_draft: false,
                }
            }),
            serialize_only: config.serialize_only(),
            verbose,
            priority_lamports: config.priority_lamports()?,
            skip_preflight: config.skip_preflight(),
            luts: config.alts().copied().collect(),
            output_format: config.output(),
        })
    }

    pub(self) fn send_bundle_options(&self) -> SendBundleOptions {
        SendBundleOptions {
            compute_unit_min_priority_lamports: Some(self.priority_lamports),
            config: RpcSendTransactionConfig {
                skip_preflight: self.skip_preflight,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub(crate) async fn send_or_serialize_with_callback(
        &self,
        mut bundle: BundleBuilder<'_, LocalSignerRef>,
        callback: impl FnOnce(
            Vec<WithSlot<Signature>>,
            Option<gmsol_sdk::Error>,
            usize,
        ) -> gmsol_sdk::Result<()>,
    ) -> gmsol_sdk::Result<()> {
        let serialize_only = self.serialize_only;
        let luts = bundle.luts_mut();
        for lut in self.luts.iter() {
            if !luts.contains_key(lut) {
                if let Some(lut) = self.alt(lut).await? {
                    luts.add(&lut);
                }
            }
        }
        let cache = luts.clone();
        if let Some(format) = serialize_only {
            let txns = to_transactions(bundle.build()?)?;
            let items = txns
                .into_iter()
                .enumerate()
                .map(|(idx, rpc)| {
                    Ok(serde_json::json!({
                        "index": idx,
                        "message": serialize_message(&rpc.message, format)?,
                    }))
                })
                .collect::<gmsol_sdk::Result<Vec<_>>>()?;
            let out = self
                .output_format
                .display_many(
                    items,
                    DisplayOptions::table_projection([("index", "Index"), ("message", "Message")]),
                )
                .map_err(gmsol_sdk::Error::custom)?;
            println!("{out}");
        } else if let Some(IxBufferCtx {
            buffer,
            client,
            is_draft,
        }) = self.ix_buffer_ctx.as_ref()
        {
            let tg = bundle.build()?.into_group();
            let ags = tg.groups().iter().flat_map(|pg| pg.iter());

            let mut bundle = client.bundle();
            bundle.luts_mut().extend(cache);
            let len = tg.len();
            let steps = len + 1;
            for (txn_idx, txn) in ags.enumerate() {
                match buffer {
                    InstructionBuffer::Timelock { role } => {
                        if *is_draft {
                            tracing::warn!(
                                "draft timelocked instruction buffer is not supported currently"
                            );
                        }

                        tracing::info!("Creating instruction buffers for transaction {txn_idx}");

                        for (idx, ix) in txn
                            .instructions_with_options(GetInstructionsOptions {
                                compute_budget: ComputeBudgetOptions {
                                    without_compute_budget: true,
                                    ..Default::default()
                                },
                                ..Default::default()
                            })
                            .enumerate()
                        {
                            let buffer = Keypair::new();
                            let (rpc, buffer) = client
                                .create_timelocked_instruction(
                                    &self.store,
                                    role,
                                    buffer,
                                    (*ix).clone(),
                                )?
                                .swap_output(());
                            bundle.push(rpc)?;
                            let out = self
                                .output_format
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
                        }
                    }
                    #[cfg(feature = "squads")]
                    InstructionBuffer::Squads {
                        multisig,
                        vault_index,
                    } => {
                        use gmsol_sdk::client::squads::{SquadsOps, VaultTransactionOptions};
                        use gmsol_sdk::solana_utils::utils::inspect_transaction;

                        let luts = tg.luts();
                        let message = txn.message_with_blockhash_and_options(
                            Default::default(),
                            GetInstructionsOptions {
                                compute_budget: ComputeBudgetOptions {
                                    without_compute_budget: true,
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            Some(luts),
                        )?;

                        let (rpc, transaction) = client
                            .squads_create_vault_transaction_with_message(
                                multisig,
                                *vault_index,
                                &message,
                                VaultTransactionOptions {
                                    draft: *is_draft,
                                    ..Default::default()
                                },
                                Some(txn_idx as u64),
                            )
                            .await?
                            .swap_output(());

                        let txn_count = txn_idx + 1;
                        let out = self.output_format
                            .display_many(
                                [serde_json::json!({
                                    "index": txn_idx,
                                    "transaction": transaction,
                                    "inspector_url": inspect_transaction(&message, Some(client.cluster()), false),
                                })],
                                DisplayOptions::table_projection([
                                    ("index", "Index"),
                                    ("transaction", "Transaction"),
                                    ("inspector_url", "Inspector URL"),
                                ]),
                            )
                            .map_err(gmsol_sdk::Error::custom)?;
                        println!("{out}");

                        let confirmation = dialoguer::Confirm::new()
                            .with_prompt(format!(
                            "[{txn_count}/{steps}] Confirm to add vault transaction {txn_idx} ?"
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
                }
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
            self.send_bundle_with_callback(bundle, callback).await?;
        } else {
            self.send_bundle_with_callback(bundle, callback).await?;
        }
        Ok(())
    }

    pub(crate) async fn send_or_serialize(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
    ) -> gmsol_sdk::Result<()> {
        self.send_or_serialize_with_callback(bundle, |s, e, steps| {
            self.display_signatures(s, e, steps)
        })
        .await
    }

    #[cfg(feature = "squads")]
    pub(crate) fn squads_ctx(&self) -> Option<(Pubkey, u8)> {
        let ix_buffer_ctx = self.ix_buffer_ctx.as_ref()?;
        if let InstructionBuffer::Squads {
            multisig,
            vault_index,
        } = ix_buffer_ctx.buffer
        {
            Some((multisig, vault_index))
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub(crate) fn host_client(&self) -> &Client<LocalSignerRef> {
        if let Some(ix_buffer_ctx) = self.ix_buffer_ctx.as_ref() {
            &ix_buffer_ctx.client
        } else {
            &self.client
        }
    }

    async fn send_bundle_with_callback(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
        callback: impl FnOnce(
            Vec<WithSlot<Signature>>,
            Option<gmsol_sdk::Error>,
            usize,
        ) -> gmsol_sdk::Result<()>,
    ) -> gmsol_sdk::Result<()> {
        let mut idx = 0;
        let bundle = bundle.build()?;
        let steps = bundle.len();
        match bundle
            .send_all_with_opts(self.send_bundle_options(), |message| {
                use gmsol_sdk::solana_utils::solana_sdk::hash::hash;
                let message_str = if self.verbose {
                    Some(inspect_transaction(message, None, true))
                } else {
                    None
                };
                if let Ok(out) = self.output_format.display_many(
                    [serde_json::json!({
                        "step": idx + 1,
                        "steps": steps,
                        "index": idx,
                        "hash": hash(&message.serialize()).to_string(),
                        "message": message_str,
                    })],
                    DisplayOptions::table_projection([
                        ("step", "Step"),
                        ("steps", "Steps"),
                        ("index", "Index"),
                        ("hash", "Hash"),
                        ("message", "Message"),
                    ]),
                ) {
                    println!("{out}");
                }
                idx += 1;
                Ok(())
            })
            .await
        {
            Ok(signatures) => (callback)(signatures, None, steps)?,
            Err((signatures, error)) => (callback)(signatures, Some(error.into()), steps)?,
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn send_bundle(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
    ) -> gmsol_sdk::Result<()> {
        self.send_bundle_with_callback(bundle, |s, e, steps| self.display_signatures(s, e, steps))
            .await
    }

    fn display_signatures(
        &self,
        signatures: Vec<WithSlot<Signature>>,
        err: Option<gmsol_sdk::Error>,
        steps: usize,
    ) -> gmsol_sdk::Result<()> {
        let failed_start = signatures.len();
        let failed = steps.saturating_sub(signatures.len());
        let mut items = Vec::new();
        for (idx, signature) in signatures.into_iter().enumerate() {
            items.push(serde_json::json!({
                "index": idx,
                "signature": signature.value(),
                "status": "ok",
            }));
        }
        for idx in 0..failed {
            items.push(serde_json::json!({
                "index": idx + failed_start,
                "status": "failed",
            }));
        }
        let out = self
            .output_format
            .display_many(
                items,
                DisplayOptions::table_projection([
                    ("index", "Index"),
                    ("signature", "Signature"),
                    ("status", "Status"),
                ]),
            )
            .map_err(gmsol_sdk::Error::custom)?;
        println!("{out}");
        match err {
            None => Ok(()),
            Some(err) => Err(err),
        }
    }
}

impl Deref for CommandClient {
    type Target = Client<LocalSignerRef>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

fn to_transactions(
    bundle: Bundle<'_, LocalSignerRef>,
) -> gmsol_sdk::Result<Vec<VersionedTransaction>> {
    let bundle = bundle.into_group();
    bundle
        .to_transactions_with_options::<Arc<NullSigner>, _>(
            &Default::default(),
            Default::default(),
            true,
            ComputeBudgetOptions {
                without_compute_budget: true,
                ..Default::default()
            },
            default_before_sign,
        )
        .flat_map(|txns| match txns {
            Ok(txns) => Either::Left(txns.into_iter().map(Ok)),
            Err(err) => Either::Right(std::iter::once(Err(err.into()))),
        })
        .collect()
}
