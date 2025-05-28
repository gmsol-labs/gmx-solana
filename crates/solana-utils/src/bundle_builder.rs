use std::{collections::HashSet, ops::Deref};

use futures_util::TryStreamExt;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, message::VersionedMessage, packet::PACKET_DATA_SIZE,
    signature::Signature, signer::Signer, transaction::VersionedTransaction,
};

use crate::{
    client::SendAndConfirm,
    cluster::Cluster,
    transaction_builder::{default_before_sign, TransactionBuilder},
    utils::{inspect_transaction, transaction_size, WithSlot},
};

const TRANSACTION_SIZE_LIMIT: usize = PACKET_DATA_SIZE;
const DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX: usize = 14;

/// Bundle Options.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Whether to force one transaction.
    pub force_one_transaction: bool,
    /// Max packet size.
    pub max_packet_size: Option<usize>,
    /// Max number of instructions for one transaction.
    pub max_instructions_for_one_tx: usize,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self {
            force_one_transaction: false,
            max_packet_size: None,
            max_instructions_for_one_tx: DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX,
        }
    }
}

/// Create Bundle Options.
#[derive(Debug, Clone, Default)]
pub struct CreateBundleOptions {
    /// Cluster.
    pub cluster: Cluster,
    /// Commitment config.
    pub commitment: CommitmentConfig,
    /// Bundle options.
    pub options: BundleOptions,
}

/// Send Bundle Options.
#[derive(Debug, Clone, Default)]
pub struct SendBundleOptions {
    /// Whether to send without compute budget.
    pub without_compute_budget: bool,
    /// Set the compute unit price.
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Set the min priority lamports.
    /// `None` means the value is left unchanged.
    pub compute_unit_min_priority_lamports: Option<u64>,
    /// Whether to continue on error.
    pub continue_on_error: bool,
    /// RPC config.
    pub config: RpcSendTransactionConfig,
    /// Whether to trace transaction error.
    pub disable_error_tracing: bool,
    /// Cluster of the inspector url.
    pub inspector_cluster: Option<Cluster>,
}

/// Buidler for transaction bundle.
pub struct BundleBuilder<'a, C> {
    client: RpcClient,
    builders: Vec<TransactionBuilder<'a, C>>,
    options: BundleOptions,
}

impl<C> BundleBuilder<'_, C> {
    /// Create a new [`BundleBuilder`] for the given cluster.
    pub fn new(cluster: Cluster) -> Self {
        Self::new_with_options(CreateBundleOptions {
            cluster,
            ..Default::default()
        })
    }

    /// Create a new [`BundleBuilder`] with the given options.
    pub fn new_with_options(options: CreateBundleOptions) -> Self {
        let rpc = options.cluster.rpc(options.commitment);

        Self::from_rpc_client_with_options(rpc, options.options)
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`].
    pub fn from_rpc_client(client: RpcClient) -> Self {
        Self::from_rpc_client_with_options(client, Default::default())
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`] with the given options.
    pub fn from_rpc_client_with_options(client: RpcClient, options: BundleOptions) -> Self {
        Self {
            client,
            builders: Default::default(),
            options,
        }
    }

    /// Get packet size.
    pub fn packet_size(&self) -> usize {
        match self.options.max_packet_size {
            Some(size) => size.min(TRANSACTION_SIZE_LIMIT),
            None => TRANSACTION_SIZE_LIMIT,
        }
    }

    /// Get the client.
    pub fn client(&self) -> &RpcClient {
        &self.client
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }

    /// Get total number of transactions.
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// Try clone empty.
    pub fn try_clone_empty(&self) -> crate::Result<Self> {
        let cluster = self.client.url().parse()?;
        let commitment = self.client.commitment();
        Ok(Self::new_with_options(CreateBundleOptions {
            cluster,
            commitment,
            options: self.options.clone(),
        }))
    }

    /// Set options.
    pub fn set_options(&mut self, options: BundleOptions) -> &mut Self {
        self.options = options;
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> BundleBuilder<'a, C> {
    /// Push a [`TransactionBuilder`] with options.
    #[allow(clippy::result_large_err)]
    pub fn try_push_with_opts(
        &mut self,
        mut txn: TransactionBuilder<'a, C>,
        new_transaction: bool,
    ) -> Result<&mut Self, (TransactionBuilder<'a, C>, crate::Error)> {
        let packet_size = self.packet_size();
        let mut ix = txn.instructions_with_options(true, None);
        let incoming_lookup_table = txn.get_complete_lookup_table();
        if transaction_size(
            txn.get_payer(),
            &ix,
            true,
            Some(&incoming_lookup_table),
            txn.get_luts().len(),
        ) > packet_size
        {
            return Err((
                txn,
                crate::Error::AddTransaction("the size of this instruction is too big"),
            ));
        }
        if self.builders.is_empty() || new_transaction {
            tracing::debug!("adding to a new tx");
            if !self.builders.is_empty() && self.options.force_one_transaction {
                return Err((txn, crate::Error::AddTransaction("cannot create more than one transaction because `force_one_transaction` is set")));
            }
            self.builders.push(txn);
        } else {
            let last = self.builders.last_mut().unwrap();

            let mut ixs_after_merge = last.instructions_with_options(false, None);
            ixs_after_merge.append(&mut ix);

            let mut lookup_table = last.get_complete_lookup_table();
            lookup_table.extend(incoming_lookup_table);
            let mut lookup_table_addresses = last.get_luts().keys().collect::<HashSet<_>>();
            lookup_table_addresses.extend(txn.get_luts().keys());

            let size_after_merge = transaction_size(
                last.get_payer(),
                &ixs_after_merge,
                true,
                Some(&lookup_table),
                lookup_table_addresses.len(),
            );
            if size_after_merge <= packet_size
                && ixs_after_merge.len() <= self.options.max_instructions_for_one_tx
            {
                tracing::debug!(size_after_merge, "adding to the last tx");
                last.try_merge(&mut txn).map_err(|err| (txn, err))?;
            } else {
                tracing::debug!(
                    size_after_merge,
                    "exceed packet data size limit, adding to a new tx"
                );
                if self.options.force_one_transaction {
                    return Err((txn, crate::Error::AddTransaction("cannot create more than one transaction because `force_one_transaction` is set")));
                }
                self.builders.push(txn);
            }
        }
        Ok(self)
    }

    /// Try to push a [`TransactionBuilder`] to the builder.
    #[allow(clippy::result_large_err)]
    #[inline]
    pub fn try_push(
        &mut self,
        txn: TransactionBuilder<'a, C>,
    ) -> Result<&mut Self, (TransactionBuilder<'a, C>, crate::Error)> {
        self.try_push_with_opts(txn, false)
    }

    /// Push a [`TransactionBuilder`].
    pub fn push(&mut self, txn: TransactionBuilder<'a, C>) -> crate::Result<&mut Self> {
        self.try_push(txn).map_err(|(_, err)| err)
    }

    /// Push [`TransactionBuilder`]s.
    pub fn push_many(
        &mut self,
        txns: impl IntoIterator<Item = TransactionBuilder<'a, C>>,
        new_transaction: bool,
    ) -> crate::Result<&mut Self> {
        for (idx, txn) in txns.into_iter().enumerate() {
            self.try_push_with_opts(txn, (idx == 0) && new_transaction)
                .map_err(|(_, err)| err)?;
        }
        Ok(self)
    }

    /// Get back all collected [`TransactionBuilder`]s.
    pub fn into_builders(self) -> Vec<TransactionBuilder<'a, C>> {
        self.builders
    }

    /// Send all in order and returns the signatures of the success transactions.
    pub async fn send_all(
        self,
        skip_preflight: bool,
    ) -> Result<Vec<Signature>, (Vec<Signature>, crate::Error)> {
        match self
            .send_all_with_opts(
                SendBundleOptions {
                    config: RpcSendTransactionConfig {
                        skip_preflight,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                default_before_sign,
            )
            .await
        {
            Ok(signatures) => Ok(signatures
                .into_iter()
                .map(|with_slot| with_slot.into_value())
                .collect()),
            Err((signatures, err)) => Err((
                signatures
                    .into_iter()
                    .map(|with_slot| with_slot.into_value())
                    .collect(),
                err,
            )),
        }
    }

    /// Send all in order with the given options and returns the signatures of the success transactions.
    pub async fn send_all_with_opts(
        self,
        opts: SendBundleOptions,
        mut before_sign: impl FnMut(&VersionedMessage) -> crate::Result<()>,
    ) -> Result<Vec<WithSlot<Signature>>, (Vec<WithSlot<Signature>>, crate::Error)> {
        let SendBundleOptions {
            without_compute_budget,
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
            continue_on_error,
            mut config,
            disable_error_tracing,
            inspector_cluster,
        } = opts;
        config.preflight_commitment = config
            .preflight_commitment
            .or(Some(self.client.commitment().commitment));
        let latest_hash = self
            .client
            .get_latest_blockhash()
            .await
            .map_err(|err| (vec![], Box::new(err).into()))?;
        let txs = self
            .builders
            .into_iter()
            .enumerate()
            .map(|(idx, mut builder)| {
                tracing::debug!(
                    size = builder.transaction_size(true),
                    "signing transaction {idx}"
                );

                if let Some(lamports) = compute_unit_min_priority_lamports {
                    builder
                        .compute_budget_mut()
                        .set_min_priority_lamports(Some(lamports));
                }

                builder.signed_transaction_with_blockhash_and_options(
                    latest_hash,
                    without_compute_budget,
                    compute_unit_price_micro_lamports,
                    &mut before_sign,
                )
            })
            .collect::<crate::Result<Vec<_>>>()
            .map_err(|err| (vec![], err))?;
        send_all_txs(
            &self.client,
            txs,
            config,
            continue_on_error,
            !disable_error_tracing,
            inspector_cluster,
        )
        .await
    }

    /// Estimate execution fee.
    pub async fn estimate_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<u64> {
        self.builders
            .iter()
            .map(|txn| txn.estimate_execution_fee(&self.client, compute_unit_price_micro_lamports))
            .collect::<futures_util::stream::FuturesUnordered<_>>()
            .try_fold(0, |acc, fee| futures_util::future::ready(Ok(acc + fee)))
            .await
    }

    /// Insert all the instructions of `other` into `self`.
    ///
    /// If `new_transaction` is `true`, then a new transaction will be created before pushing.
    pub fn append(&mut self, other: Self, new_transaction: bool) -> crate::Result<()> {
        let builders = other.into_builders();

        for (idx, txn) in builders.into_iter().enumerate() {
            self.try_push_with_opts(txn, new_transaction && idx == 0)
                .map_err(|(_, err)| err)?;
        }

        Ok(())
    }
}

async fn send_all_txs(
    client: &RpcClient,
    txs: impl IntoIterator<Item = VersionedTransaction>,
    config: RpcSendTransactionConfig,
    continue_on_error: bool,
    enable_tracing: bool,
    inspector_cluster: Option<Cluster>,
) -> Result<Vec<WithSlot<Signature>>, (Vec<WithSlot<Signature>>, crate::Error)> {
    let txs = txs.into_iter();
    let (min, max) = txs.size_hint();
    let mut signatures = Vec::with_capacity(max.unwrap_or(min));
    let mut error = None;
    for (idx, tx) in txs.into_iter().enumerate() {
        tracing::debug!(
            commitment = ?client.commitment(),
            ?config,
            "sending transaction {idx}"
        );
        match client
            .send_and_confirm_transaction_with_config(&tx, config)
            .await
        {
            Ok(signature) => {
                signatures.push(signature);
            }
            Err(err) => {
                if enable_tracing {
                    let cluster = inspector_cluster
                        .clone()
                        .or_else(|| client.url().parse().ok());
                    let inspector_url = inspect_transaction(&tx.message, cluster.as_ref(), false);
                    let hash = tx.message.recent_blockhash();
                    tracing::error!(%err, %hash, ?config, "transaction {idx} failed: {inspector_url}");
                }

                error = Some(Box::new(err).into());
                if !continue_on_error {
                    break;
                }
            }
        }
    }
    match error {
        None => Ok(signatures),
        Some(err) => Err((signatures, err)),
    }
}

impl<'a, C> IntoIterator for BundleBuilder<'a, C> {
    type Item = TransactionBuilder<'a, C>;

    type IntoIter = <Vec<TransactionBuilder<'a, C>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.builders.into_iter()
    }
}
