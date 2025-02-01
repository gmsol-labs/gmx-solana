use std::{collections::HashMap, fs, ops::Deref, path::PathBuf, rc::Rc};

use anchor_client::solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use eyre::OptionExt;
use gmsol::{timelock::TimelockOps, utils::instruction::InstructionSerialization};
use gmsol_solana_utils::{bundle_builder::BundleBuilder, transaction_builder::TransactionBuilder};
use prettytable::format::{FormatBuilder, TableFormat};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use url::Url;

use crate::GMSOLClient;

pub(crate) type InstructionBufferCtx<'a> = (InstructionBuffer<'a>, &'a GMSOLClient, bool);

#[derive(clap::ValueEnum, Clone, Copy, Default)]
#[clap(rename_all = "kebab-case")]
pub(crate) enum Output {
    /// Text.
    #[default]
    Text,
    /// Json.
    Json,
    /// Json Compact.
    JsonCompact,
}

impl Output {
    pub(crate) fn print<T: serde::Serialize>(
        &self,
        value: &T,
        text: impl FnOnce(&T) -> gmsol::Result<String>,
    ) -> gmsol::Result<()> {
        match self {
            Self::Text => {
                println!("{}", text(value)?);
            }
            Self::Json => {
                println!("{}", serde_json::to_string_pretty(value)?);
            }
            Self::JsonCompact => {
                println!("{}", serde_json::to_string(value)?);
            }
        }
        Ok(())
    }
}

pub(crate) fn generate_discriminator(
    name: &str,
    namespace: Option<&str>,
    force_snake_case: bool,
) -> [u8; 8] {
    use anchor_syn::codegen::program::common::{sighash, SIGHASH_GLOBAL_NAMESPACE};
    use heck::AsSnakeCase;

    let snake_case = AsSnakeCase(name).to_string();
    sighash(
        namespace.unwrap_or(SIGHASH_GLOBAL_NAMESPACE),
        if force_snake_case { &snake_case } else { name },
    )
}

pub(crate) enum InstructionBuffer<'a> {
    Timelock {
        role: &'a str,
    },
    #[cfg(feature = "squads")]
    Squads {
        multisig: Pubkey,
        vault_index: u8,
    },
}

pub(crate) fn instruction_buffer_not_supported(
    ctx: Option<InstructionBufferCtx<'_>>,
) -> gmsol::Result<()> {
    if ctx.is_some() {
        Err(gmsol::Error::invalid_argument(
            "instruction buffer is not supported",
        ))
    } else {
        Ok(())
    }
}

pub(crate) async fn send_or_serialize_transaction<C, S>(
    store: &Pubkey,
    rpc: TransactionBuilder<'_, C>,
    instruction_buffer_ctx: Option<InstructionBufferCtx<'_>>,
    serialize_only: Option<InstructionSerialization>,
    skip_preflight: bool,
    callback: impl FnOnce(Signature) -> gmsol::Result<()>,
) -> gmsol::Result<()>
where
    C: Clone + Deref<Target = S>,
    S: Signer,
{
    let bundle = rpc.into_bundle_with_options(true, None, None)?;
    send_or_serialize_bundle(
        store,
        bundle,
        instruction_buffer_ctx,
        serialize_only,
        skip_preflight,
        |mut signatures, err| match err {
            Some(err) => Err(err),
            None => {
                debug_assert_eq!(signatures.len(), 1, "force one transaction");
                let signature = signatures.pop().expect("must exist");
                (callback)(signature)
            }
        },
    )
    .await
}

pub(crate) async fn send_or_serialize_bundle<C, S>(
    store: &Pubkey,
    builder: BundleBuilder<'_, C>,
    instruction_buffer_ctx: Option<InstructionBufferCtx<'_>>,
    serialize_only: Option<InstructionSerialization>,
    skip_preflight: bool,
    callback: impl FnOnce(Vec<Signature>, Option<gmsol::Error>) -> gmsol::Result<()>,
) -> gmsol::Result<()>
where
    C: Clone + Deref<Target = S>,
    S: Signer,
{
    if let Some(format) = serialize_only {
        for (idx, rpc) in builder.into_builders().into_iter().enumerate() {
            println!("Transaction {idx}:");
            let payer_address = rpc.get_payer();
            for (idx, ix) in rpc
                .instructions_with_options(true, None)
                .into_iter()
                .enumerate()
            {
                println!(
                    "ix[{idx}]: {}",
                    gmsol::utils::serialize_instruction(&ix, format, Some(&payer_address))?
                );
            }
            println!();
        }
    } else if let Some((instruction_buffer, client, draft)) = instruction_buffer_ctx {
        let mut bundle = client.bundle();

        let txns = builder.into_builders();
        let len = txns.len();
        let steps = len + 1;
        for (txn_idx, txn) in txns.into_iter().enumerate() {
            match instruction_buffer {
                InstructionBuffer::Timelock { role } => {
                    if draft {
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
                            .create_timelocked_instruction(store, role, buffer, ix)?
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
                    use gmsol::squads::SquadsOps;
                    use gmsol_solana_utils::utils::inspect_transaction;

                    let message =
                        txn.message_with_blockhash_and_options(Default::default(), true, None)?;

                    let (rpc, transaction) = client
                        .squads_create_vault_transaction(
                            &multisig,
                            vault_index,
                            &message,
                            None,
                            draft,
                        )
                        .await?
                        .swap_output(());

                    let txn_count = txn_idx + 1;
                    tracing::info!(
                        %transaction,
                        %draft,
                        "Adding a vault transaction {txn_idx}: {}",

                        inspect_transaction(&message, Some(client.cluster()), false),
                    );

                    let confirmation = dialoguer::Confirm::new()
                        .with_prompt(format!(
                            "[{txn_count}/{steps}] Confirm to add vault transaction {txn_idx} ?"
                        ))
                        .default(false)
                        .interact()
                        .map_err(gmsol::Error::unknown)?;

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
            .map_err(gmsol::Error::unknown)?;

        if !confirmation {
            tracing::info!("Cancelled");
            return Ok(());
        }

        match bundle.send_all(skip_preflight).await {
            Ok(signatures) => {
                tracing::info!("successful transactions: {signatures:#?}");
            }
            Err((signatures, error)) => {
                tracing::error!(%error, "successful transactions: {signatures:#?}");
            }
        }
    } else {
        match builder.send_all(skip_preflight).await {
            Ok(signatures) => (callback)(signatures, None)?,
            Err((signatures, error)) => (callback)(signatures, Some(error.into()))?,
        }
    }
    Ok(())
}

pub(crate) fn table_format() -> TableFormat {
    use prettytable::format::{LinePosition, LineSeparator};

    FormatBuilder::new()
        .padding(0, 2)
        .separator(LinePosition::Title, LineSeparator::new('-', '+', '+', '+'))
        .build()
}

/// Side.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum Side {
    /// Long.
    Long,
    /// Short.
    Short,
}

impl Side {
    /// Is long side.
    pub fn is_long(&self) -> bool {
        matches!(self, Self::Long)
    }
}

/// Parse url or path.
pub fn parse_url_or_path(source: &str) -> eyre::Result<Url> {
    let url = match Url::parse(source) {
        Ok(url) => url,
        Err(_) => {
            let path = shellexpand::tilde(source);
            let path: PathBuf = path.parse()?;
            let path = fs::canonicalize(path)?;
            Url::from_file_path(&path).expect("must be valid file path")
        }
    };

    Ok(url)
}

/// Load signer from url.
pub fn signer_from_source(
    source: &str,
    confirm_key: bool,
    keypair_name: &str,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> eyre::Result<gmsol::utils::LocalSignerRef> {
    const QUERY_KEY: &str = "key";

    use anchor_client::solana_sdk::{
        derivation_path::DerivationPath, signature::read_keypair_file,
    };
    use gmsol::utils::local_signer;
    use solana_remote_wallet::{
        locator::Locator, remote_keypair::generate_remote_keypair,
        remote_wallet::maybe_wallet_manager,
    };

    let url = parse_url_or_path(source)?;

    match url.scheme() {
        "file" => {
            let keypair = read_keypair_file(url.path()).map_err(|err| eyre::eyre!("{err}"))?;
            Ok(local_signer(keypair))
        }
        "usb" => {
            let manufacturer = url.host_str().ok_or_eyre("missing manufacturer")?;
            let path = url.path();
            let path = path.strip_prefix('/').unwrap_or(path);
            let pubkey = (!path.is_empty()).then_some(path);
            let locator = Locator::new_from_parts(manufacturer, pubkey)?;
            let query = url.query_pairs().collect::<HashMap<_, _>>();
            if query.len() > 1 {
                eyre::bail!("invalid query string, extra fields not supported");
            }
            let derivation_path = query
                .get(QUERY_KEY)
                .map(|value| DerivationPath::from_key_str(value))
                .transpose()?;
            if wallet_manager.is_none() {
                *wallet_manager = maybe_wallet_manager()?;
            }
            let wallet_manager = wallet_manager.as_ref().ok_or_eyre("no device found")?;
            let keypair = generate_remote_keypair(
                locator,
                derivation_path.unwrap_or_default(),
                wallet_manager,
                confirm_key,
                keypair_name,
            )?;
            Ok(local_signer(keypair))
        }
        scheme => Err(eyre::eyre!("unsupported scheme: {scheme}")),
    }
}

#[derive(clap::Args, Clone)]
pub(crate) struct SelectGtExchangeVaultByDate {
    #[arg(long, short)]
    date: Option<humantime::Timestamp>,
}

impl SelectGtExchangeVaultByDate {
    pub(crate) async fn get(&self, store: &Pubkey, client: &GMSOLClient) -> gmsol::Result<Pubkey> {
        use std::time::SystemTime;

        let time_window = client.store(store).await?.gt().exchange_time_window();
        let date = self
            .date
            .as_ref()
            .cloned()
            .unwrap_or_else(|| humantime::Timestamp::from(SystemTime::now()));
        let ts = date
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(gmsol::Error::unknown)?
            .as_secs();
        let index = ts / time_window as u64;
        Ok(client.find_gt_exchange_vault_address(store, index as i64, time_window))
    }
}

#[derive(clap::Args, Clone)]
pub(crate) struct SelectGtExchangeVault {
    gt_exchange_vault: Option<Pubkey>,
    #[clap(flatten)]
    date: SelectGtExchangeVaultByDate,
}

impl SelectGtExchangeVault {
    pub(crate) async fn get(&self, store: &Pubkey, client: &GMSOLClient) -> gmsol::Result<Pubkey> {
        if let Some(address) = self.gt_exchange_vault {
            Ok(address)
        } else {
            self.date.get(store, client).await
        }
    }
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ToggleValue {
    #[arg(long)]
    enable: bool,
    #[arg(long)]
    disable: bool,
}

impl ToggleValue {
    pub(crate) fn is_enable(&self) -> bool {
        debug_assert!(self.enable != self.disable);
        self.enable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url_or_path() -> eyre::Result<()> {
        let path = "~/.config/solana/id.json";
        assert!(parse_url_or_path(path).is_ok());
        Ok(())
    }
}
