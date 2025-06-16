use std::{ops::Deref, path::Path};

use admin::Admin;
use alt::Alt;
use competition::Competition;
use configuration::Configuration;
use enum_dispatch::enum_dispatch;
use exchange::Exchange;
use eyre::OptionExt;
use get_pubkey::GetPubkey;
use glv::Glv;
use gmsol_sdk::{
    ops::TimelockOps,
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        bundle_builder::{BundleBuilder, BundleOptions, SendBundleOptions},
        signer::LocalSignerRef,
        solana_client::rpc_config::RpcSendTransactionConfig,
        solana_sdk::{
            message::VersionedMessage,
            signature::{Keypair, Signature},
        },
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
use treasury::Treasury;
use user::User;

use crate::config::{Config, InstructionBuffer, Payer};

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
        bundle: BundleBuilder<'_, LocalSignerRef>,
        callback: impl FnOnce(
            Vec<WithSlot<Signature>>,
            Option<gmsol_sdk::Error>,
            usize,
        ) -> gmsol_sdk::Result<()>,
    ) -> gmsol_sdk::Result<()> {
        let options = self.send_bundle_options();
        let serialize_only = self.serialize_only;
        if let Some(format) = serialize_only {
            println!("\n[Transactions]");
            for (idx, rpc) in bundle.into_builders().into_iter().enumerate() {
                let message =
                    rpc.message_with_blockhash_and_options(Default::default(), true, None)?;
                println!("TXN[{idx}]: {}", serialize_message(&message, format)?);
            }
        } else if let Some(IxBufferCtx {
            buffer,
            client,
            is_draft,
        }) = self.ix_buffer_ctx.as_ref()
        {
            let txns = bundle.into_builders();

            let mut bundle = client.bundle();
            let len = txns.len();
            let steps = len + 1;
            for (txn_idx, txn) in txns.into_iter().enumerate() {
                match buffer {
                    InstructionBuffer::Timelock { role } => {
                        if *is_draft {
                            tracing::warn!(
                                "draft timelocked instruction buffer is not supported currently"
                            );
                        }

                        tracing::info!("Creating instruction buffers for transaction {txn_idx}");

                        for (idx, ix) in txn
                            .instructions_with_options(true, None)
                            .into_iter()
                            .enumerate()
                        {
                            let buffer = Keypair::new();
                            let (rpc, buffer) = client
                                .create_timelocked_instruction(&self.store, role, buffer, ix)?
                                .swap_output(());
                            bundle.push(rpc)?;
                            println!("ix[{idx}]: {buffer}");
                        }
                    }
                    #[cfg(feature = "squads")]
                    InstructionBuffer::Squads {
                        multisig,
                        vault_index,
                    } => {
                        use gmsol_sdk::client::squads::SquadsOps;
                        use gmsol_sdk::solana_utils::utils::inspect_transaction;

                        let message =
                            txn.message_with_blockhash_and_options(Default::default(), true, None)?;

                        let (rpc, transaction) = client
                            .squads_create_vault_transaction(
                                multisig,
                                *vault_index,
                                &message,
                                None,
                                *is_draft,
                                Some(txn_idx as u64),
                            )
                            .await?
                            .swap_output(());

                        let txn_count = txn_idx + 1;
                        println!("Adding a vault transaction {txn_idx}: id = {}", transaction,);
                        println!(
                            "Inspector URL for transaction {txn_idx}: {}",
                            inspect_transaction(&message, Some(client.cluster()), false),
                        );

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

            let mut idx = 0;
            let steps = bundle.len();
            let (signatures, err) = match bundle
                .send_all_with_opts(options, |m| before_sign(&mut idx, steps, self.verbose, m))
                .await
            {
                Ok(signatures) => {
                    tracing::info!("successful transactions: {signatures:#?}");
                    (signatures, None)
                }
                Err((signatures, error)) => {
                    tracing::error!(%error, "successful transactions: {signatures:#?}");
                    (signatures, Some(error))
                }
            };
            display_signatures(signatures, err.map(Into::into), steps)?;
        } else {
            let mut idx = 0;
            let steps = bundle.len();
            match bundle
                .send_all_with_opts(options, |m| before_sign(&mut idx, steps, self.verbose, m))
                .await
            {
                Ok(signatures) => (callback)(signatures, None, steps)?,
                Err((signatures, error)) => (callback)(signatures, Some(error.into()), steps)?,
            }
        }
        Ok(())
    }

    pub(crate) async fn send_or_serialize(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
    ) -> gmsol_sdk::Result<()> {
        self.send_or_serialize_with_callback(bundle, display_signatures)
            .await
    }
}

impl Deref for CommandClient {
    type Target = Client<LocalSignerRef>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

fn before_sign(
    idx: &mut usize,
    steps: usize,
    verbose: bool,
    message: &VersionedMessage,
) -> Result<(), gmsol_sdk::SolanaUtilsError> {
    use gmsol_sdk::solana_utils::solana_sdk::hash::hash;
    println!(
        "[{}/{steps}] Signing transaction {idx}: hash = {}{}",
        *idx + 1,
        hash(&message.serialize()),
        if verbose {
            format!(", message = {}", inspect_transaction(message, None, true))
        } else {
            String::new()
        }
    );
    *idx += 1;

    Ok(())
}

fn display_signatures(
    signatures: Vec<WithSlot<Signature>>,
    err: Option<gmsol_sdk::Error>,
    steps: usize,
) -> gmsol_sdk::Result<()> {
    let failed_start = signatures.len();
    let failed = steps.saturating_sub(signatures.len());
    for (idx, signature) in signatures.into_iter().enumerate() {
        println!("Transaction {idx}: signature = {}", signature.value());
    }
    for idx in 0..failed {
        println!("Transaction {}: failed", idx + failed_start);
    }
    match err {
        None => Ok(()),
        Some(err) => Err(err),
    }
}
