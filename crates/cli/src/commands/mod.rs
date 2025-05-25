use std::{ops::Deref, path::Path};

use admin::Admin;
use competition::Competition;
use enum_dispatch::enum_dispatch;
use exchange::Exchange;
use eyre::OptionExt;
use get_pubkey::GetPubkey;
use gmsol_sdk::{
    ops::TimelockOps,
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        bundle_builder::{BundleBuilder, BundleOptions, SendBundleOptions},
        signer::LocalSignerRef,
        solana_sdk::signature::{Keypair, Signature},
    },
    utils::instruction_serialization::InstructionSerialization,
    Client,
};
use init_config::InitConfig;

#[cfg(feature = "remote-wallet")]
use solana_remote_wallet::remote_wallet::RemoteWalletManager;

use crate::config::{Config, InstructionBuffer, Payer};

mod admin;
mod competition;
mod exchange;
mod get_pubkey;
mod init_config;

/// Utils for command implementations.
pub mod utils;

/// Commands.
#[enum_dispatch]
#[derive(Debug, clap::Subcommand)]
pub enum Commands {
    /// Administrative commands.
    Admin(Admin),
    /// Initialize config file.
    InitConfig(InitConfig),
    /// Get pubkey of the payer.
    Pubkey(GetPubkey),
    /// Commands for exchange functionalities.
    Exchange(Box<Exchange>),
    /// Commands for competition management.
    Competition(Competition),
}

#[enum_dispatch(Commands)]
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
    client: Option<&'a CommandClient>,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        store: Pubkey,
        config_path: &'a Path,
        client: Option<&'a CommandClient>,
    ) -> Self {
        Self {
            store,
            config_path,
            client,
        }
    }

    pub(crate) fn client(&self) -> eyre::Result<&CommandClient> {
        self.client.ok_or_eyre("client is not provided")
    }

    pub(crate) fn store(&self) -> &Pubkey {
        &self.store
    }

    pub(crate) fn bundle_options(&self) -> BundleOptions {
        BundleOptions::default()
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
}

impl CommandClient {
    pub(crate) fn new(
        config: &Config,
        #[cfg(feature = "remote-wallet")] wallet_manager: &mut Option<
            std::rc::Rc<RemoteWalletManager>,
        >,
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
        })
    }

    pub(self) fn send_bundle_options(&self) -> SendBundleOptions {
        SendBundleOptions::default()
    }

    pub(crate) async fn send_or_serialize_with_callback(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
        callback: impl FnOnce(Vec<Signature>, Option<gmsol_sdk::Error>) -> gmsol_sdk::Result<()>,
    ) -> gmsol_sdk::Result<()> {
        use gmsol_sdk::utils::instruction_serialization::serialize_instruction;

        let options = self.send_bundle_options();
        let serialize_only = self.serialize_only;
        if let Some(format) = serialize_only {
            for (idx, rpc) in bundle.into_builders().into_iter().enumerate() {
                println!("Transaction {idx}:");
                let payer_address = rpc.get_payer();
                for (idx, ix) in rpc
                    .instructions_with_options(true, None)
                    .into_iter()
                    .enumerate()
                {
                    println!(
                        "ix[{idx}]: {}",
                        serialize_instruction(&ix, format, Some(&payer_address))?
                    );
                }
                println!();
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
                        tracing::info!(
                            %transaction,
                            %is_draft,
                            "Adding a vault transaction {txn_idx}: {}",

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

            match bundle.send_all_with_opts(options).await {
                Ok(signatures) => {
                    tracing::info!("successful transactions: {signatures:#?}");
                }
                Err((signatures, error)) => {
                    tracing::error!(%error, "successful transactions: {signatures:#?}");
                }
            }
        } else {
            match bundle.send_all_with_opts(options).await {
                Ok(signatures) => (callback)(
                    signatures.into_iter().map(|w| w.into_value()).collect(),
                    None,
                )?,
                Err((signatures, error)) => (callback)(
                    signatures.into_iter().map(|w| w.into_value()).collect(),
                    Some(error.into()),
                )?,
            }
        }
        Ok(())
    }

    pub(crate) async fn send_or_serialize(
        &self,
        bundle: BundleBuilder<'_, LocalSignerRef>,
    ) -> gmsol_sdk::Result<()> {
        self.send_or_serialize_with_callback(bundle, |signatures, err| {
            tracing::info!("{signatures:#?}");
            match err {
                None => Ok(()),
                Some(err) => Err(err),
            }
        })
        .await
    }
}

impl Deref for CommandClient {
    type Target = Client<LocalSignerRef>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}
