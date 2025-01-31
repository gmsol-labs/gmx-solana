use std::{collections::HashSet, ops::Deref};

use futures_util::TryStreamExt;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, packet::PACKET_DATA_SIZE, signature::Signature,
    signer::Signer, transaction::VersionedTransaction,
};

use crate::{
    client::SendAndConfirm,
    cluster::Cluster,
    transaction_builder::TransactionBuilder,
    utils::{inspect_transaction, transaction_size},
};

const TRANSACTION_SIZE_LIMIT: usize = PACKET_DATA_SIZE;
const DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX: usize = 14;

/// Create Bundle Options.
#[derive(Debug, Clone, Default)]
pub struct CreateBundleOptions {
    /// Cluster.
    pub cluster: Cluster,
    /// Commitment config.
    pub commitment: CommitmentConfig,
    /// Whether to force one transaction.
    pub force_one_transaction: bool,
    /// Max packet size.
    pub max_packet_size: Option<usize>,
    /// Max number of instructions for one transaction.
    pub max_instructions_for_one_tx: Option<usize>,
}

/// Send Bundle Options.
#[derive(Debug, Clone, Default)]
pub struct SendBundleOptions {
    /// Whether to send without compute budget.
    pub without_compute_budget: bool,
    /// Compute unit price.
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Whether to update recent block hash before send.
    pub update_recent_block_hash_before_send: bool,
    /// Whether to continue on error.
    pub continue_on_error: bool,
    /// RPC config.
    pub config: RpcSendTransactionConfig,
}

/// Buidler for transaction bundle.
pub struct BundleBuilder<'a, C> {
    client: RpcClient,
    builders: Vec<TransactionBuilder<'a, C>>,
    force_one_transaction: bool,
    max_packet_size: Option<usize>,
    max_instructions_for_one_tx: usize,
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

        Self::from_rpc_client_with_options(
            rpc,
            options.force_one_transaction,
            options.max_packet_size,
            options.max_instructions_for_one_tx,
        )
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`].
    pub fn from_rpc_client(client: RpcClient) -> Self {
        Self::from_rpc_client_with_options(client, false, None, None)
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`] with the given options.
    pub fn from_rpc_client_with_options(
        client: RpcClient,
        force_one_transaction: bool,
        max_packet_size: Option<usize>,
        max_instructions_for_one_tx: Option<usize>,
    ) -> Self {
        Self {
            client,
            builders: Default::default(),
            force_one_transaction,
            max_packet_size,
            max_instructions_for_one_tx: max_instructions_for_one_tx
                .unwrap_or(DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX),
        }
    }

    /// Get packet size.
    pub fn packet_size(&self) -> usize {
        match self.max_packet_size {
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
            if !self.builders.is_empty() && self.force_one_transaction {
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
                &ixs_after_merge,
                true,
                Some(&lookup_table),
                lookup_table_addresses.len(),
            );
            if size_after_merge <= packet_size
                && ixs_after_merge.len() <= self.max_instructions_for_one_tx
            {
                tracing::debug!(size_after_merge, "adding to the last tx");
                last.try_merge(&mut txn).map_err(|err| (txn, err))?;
            } else {
                tracing::debug!(
                    size_after_merge,
                    "exceed packet data size limit, adding to a new tx"
                );
                if self.force_one_transaction {
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
        self.send_all_with_opts(SendBundleOptions {
            config: RpcSendTransactionConfig {
                skip_preflight,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
    }

    /// Send all in order with the given options and returns the signatures of the success transactions.
    pub async fn send_all_with_opts(
        self,
        opts: SendBundleOptions,
    ) -> Result<Vec<Signature>, (Vec<Signature>, crate::Error)> {
        let SendBundleOptions {
            without_compute_budget,
            compute_unit_price_micro_lamports,
            update_recent_block_hash_before_send,
            continue_on_error,
            mut config,
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
            .map(|(idx, builder)| {
                tracing::debug!(
                    size = builder.transaction_size(true),
                    "signing transaction {idx}"
                );
                builder.signed_transaction_with_blockhash_and_options(
                    latest_hash,
                    without_compute_budget,
                    compute_unit_price_micro_lamports,
                )
            })
            .collect::<crate::Result<Vec<_>>>()
            .map_err(|err| (vec![], err))?;
        send_all_txs(
            &self.client,
            txs,
            config,
            update_recent_block_hash_before_send,
            continue_on_error,
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
    update_recent_block_hash_before_send: bool,
    continue_on_error: bool,
) -> Result<Vec<Signature>, (Vec<Signature>, crate::Error)> {
    let txs = txs.into_iter();
    let (min, max) = txs.size_hint();
    let mut signatures = Vec::with_capacity(max.unwrap_or(min));
    let mut error = None;
    for (idx, mut tx) in txs.into_iter().enumerate() {
        if update_recent_block_hash_before_send {
            match client.get_latest_blockhash().await {
                Ok(latest_blockhash) => {
                    tx.message.set_recent_blockhash(latest_blockhash);
                }
                Err(err) => {
                    error = Some(Box::new(err).into());
                    break;
                }
            }
        }
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
                let cluster = client.url().parse().ok().and_then(|cluster| {
                    (!matches!(cluster, Cluster::Custom(_, _))).then_some(cluster)
                });
                let inspector_url = inspect_transaction(&tx.message, cluster.as_ref(), false);
                let hash = tx.message.recent_blockhash();
                tracing::trace!(%err, %hash, ?config, "transaction failed: {inspector_url}");
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
