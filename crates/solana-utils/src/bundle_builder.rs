use std::{collections::HashMap, ops::Deref};

use futures_util::{stream::FuturesOrdered, FutureExt, StreamExt};
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, message::VersionedMessage, packet::PACKET_DATA_SIZE,
    pubkey::Pubkey, signature::Signature, signer::Signer, transaction::VersionedTransaction,
};

use crate::{
    address_lookup_table::AddressLookupTables,
    client::SendAndConfirm,
    cluster::Cluster,
    instruction_group::{AtomicGroupOptions, ComputeBudgetOptions, ParallelGroupOptions},
    signer::TransactionSigners,
    transaction_builder::{default_before_sign, TransactionBuilder},
    transaction_group::TransactionGroupOptions,
    utils::{inspect_transaction, WithSlot},
    AtomicGroup, ParallelGroup, TransactionGroup,
};

const TRANSACTION_SIZE_LIMIT: usize = PACKET_DATA_SIZE;
/// Default max instruction for one transaction.
pub const DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX: usize = 14;

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

/// Builder for transaction bundle.
pub struct BundleBuilder<'a, C> {
    ctx: Ctx<'a, C>,
    options: BundleOptions,
    groups: Vec<ParallelGroup>,
    luts: AddressLookupTables,
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

    /// Replaces the bundle options with the given.
    pub fn set_options(&mut self, options: BundleOptions) -> &mut Self {
        self.options = options;
        self
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`].
    pub fn from_rpc_client(client: RpcClient) -> Self {
        Self::from_rpc_client_with_options(client, Default::default())
    }

    /// Create a new [`BundleBuilder`] from [`RpcClient`] with the given options.
    pub fn from_rpc_client_with_options(client: RpcClient, options: BundleOptions) -> Self {
        Self {
            groups: Default::default(),
            options,
            ctx: Ctx {
                client,
                cfg_signers: Default::default(),
                signers: Default::default(),
            },
            luts: Default::default(),
        }
    }

    /// Get packet size.
    pub fn packet_size(&self) -> usize {
        self.options
            .max_packet_size
            .unwrap_or(TRANSACTION_SIZE_LIMIT)
    }

    /// Get the client.
    pub fn client(&self) -> &RpcClient {
        &self.ctx.client
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Get total number of transactions.
    pub fn len(&self) -> usize {
        self.groups.iter().map(|pg| pg.len()).sum()
    }

    /// Try clone empty.
    pub fn try_clone_empty(&self) -> crate::Result<Self> {
        let cluster = self.ctx.client.url().parse()?;
        let commitment = self.ctx.client.commitment();
        Ok(Self::new_with_options(CreateBundleOptions {
            cluster,
            commitment,
            options: self.options.clone(),
        }))
    }

    /// Push a [`ParallelGroup`].
    pub fn push_parallel_group(&mut self, group: ParallelGroup) -> &mut Self {
        if !group.is_empty() {
            self.groups.push(group);
        }
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> BundleBuilder<'a, C> {
    fn register_transaction_builder(
        &mut self,
        txn: TransactionBuilder<'a, C>,
        options: AtomicGroupOptions,
    ) -> AtomicGroup {
        txn.into_atomic_group(
            &mut self.ctx.cfg_signers,
            &mut self.ctx.signers,
            &mut self.luts,
            options,
        )
    }

    /// Push a [`TransactionBuilder`] with options.
    #[allow(clippy::result_large_err)]
    pub fn try_push_with_opts(
        &mut self,
        txn: TransactionBuilder<'a, C>,
        new_transaction: bool,
    ) -> Result<&mut Self, (TransactionBuilder<'a, C>, crate::Error)> {
        let ag = self.register_transaction_builder(
            txn,
            AtomicGroupOptions {
                is_mergeable: !new_transaction,
            },
        );
        self.push_parallel_group(ParallelGroup::with_options(
            [ag],
            ParallelGroupOptions {
                is_mergeable: !new_transaction,
            },
        ));
        Ok(self)
    }

    /// Push multiple transactions that can be sent simultaneously.
    pub fn push_parallel(&mut self) -> PushParallel<'_, 'a, C> {
        self.push_parallel_with_options(Default::default())
    }

    /// Push multiple transactions that can be sent simultaneously with the given options.
    pub fn push_parallel_with_options(
        &mut self,
        options: ParallelGroupOptions,
    ) -> PushParallel<'_, 'a, C> {
        PushParallel::new(self, options)
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

    /// Returns the transaction groups.
    pub fn into_parallel_groups(self) -> Vec<ParallelGroup> {
        self.groups
    }

    /// Insert all the transaction groups of `other` into `self`.
    ///
    /// If `new_transaction` is `true`, then a new transaction will be created before pushing.
    pub fn append(&mut self, other: Self, new_transaction: bool) -> crate::Result<()> {
        let Self {
            mut groups,
            ctx:
                Ctx {
                    mut cfg_signers,
                    signers,
                    ..
                },
            luts,
            ..
        } = other;

        if let Some(first) = groups.first_mut() {
            first.set_is_mergeable(first.is_mergeable() && !new_transaction);
        }

        self.groups.append(&mut groups);
        self.ctx.cfg_signers.merge(&mut cfg_signers);
        self.ctx.signers.extend(signers);
        self.luts.extend(luts);

        Ok(())
    }

    /// Get the reference of the address lookup table cache.
    pub fn luts(&self) -> &AddressLookupTables {
        &self.luts
    }

    /// Get the mutable reference of the address lookup table cache.
    pub fn luts_mut(&mut self) -> &mut AddressLookupTables {
        &mut self.luts
    }

    /// Build the [`Bundle`].
    pub fn build(self) -> crate::Result<Bundle<'a, C>> {
        self.build_with_options(|options| {
            Ok(TransactionGroupOptions {
                max_transaction_size: options.max_packet_size.unwrap_or(TRANSACTION_SIZE_LIMIT),
                max_instructions_per_tx: options.max_instructions_for_one_tx,
                memo: None,
                memo_signers: None,
            })
        })
    }

    /// Build [`Bundle`] with options builder `f`.
    pub fn build_with_options(
        self,
        f: impl FnOnce(BundleOptions) -> crate::Result<TransactionGroupOptions>,
    ) -> crate::Result<Bundle<'a, C>> {
        let Self {
            groups,
            options,
            ctx,
            luts,
        } = self;
        let mut group = TransactionGroup::with_options_and_luts(f(options)?, luts);
        for pg in groups {
            group.add(pg)?;
        }
        group.optimize(false);
        Ok(Bundle { ctx, group })
    }
}

struct Ctx<'a, C> {
    client: RpcClient,
    cfg_signers: TransactionSigners<C>,
    signers: HashMap<Pubkey, &'a dyn Signer>,
}

/// Push multiple transactions that can be sent simultaneously to the [`BundleBuilder`].
pub struct PushParallel<'a, 'ctx, C> {
    bundle: &'a mut BundleBuilder<'ctx, C>,
    pg: Option<ParallelGroup>,
}

impl<'a, 'ctx, C> PushParallel<'a, 'ctx, C> {
    fn new(bundle: &'a mut BundleBuilder<'ctx, C>, options: ParallelGroupOptions) -> Self {
        Self {
            bundle,
            pg: Some(ParallelGroup::with_options([], options)),
        }
    }
}

impl<'ctx, C: Deref<Target = impl Signer> + Clone> PushParallel<'_, 'ctx, C> {
    /// Add a [`TransactionBuilder`] to the parallel group with the given options.
    pub fn add_with_options(
        &mut self,
        txn: TransactionBuilder<'ctx, C>,
        options: AtomicGroupOptions,
    ) -> &mut Self {
        let ag = self.bundle.register_transaction_builder(txn, options);
        self.pg.as_mut().expect("the builder is dropped").add(ag);
        self
    }

    /// Add a [`TransactionBuilder`] to the parallel group.
    pub fn add(&mut self, txn: TransactionBuilder<'ctx, C>) -> &mut Self {
        self.add_with_options(txn, Default::default())
    }
}

impl<C> Drop for PushParallel<'_, '_, C> {
    fn drop(&mut self) {
        if let Some(pg) = self.pg.take() {
            self.bundle.push_parallel_group(pg);
        }
    }
}

/// A bundle of transactions.
pub struct Bundle<'a, C> {
    ctx: Ctx<'a, C>,
    group: TransactionGroup,
}

impl<C: Deref<Target = impl Signer> + Clone> Bundle<'_, C> {
    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.group.is_empty()
    }

    /// Get total number of transactions.
    pub fn len(&self) -> usize {
        self.group.len()
    }

    /// Returns the inner [`TransactionGroup`].
    pub fn into_group(self) -> TransactionGroup {
        self.group
    }

    /// Consumes the [`Bundle`] and runs `f` with the inner [`TransactionGroup`] and [`TransactionSigners`].
    pub fn with_inner<'a, T>(
        &'a self,
        f: impl FnOnce(&'a RpcClient, &'a TransactionGroup, TransactionSigners<&'a dyn Signer>) -> T,
    ) -> T {
        let Self { ctx, group } = self;
        let Ctx {
            client,
            cfg_signers,
            signers,
        } = ctx;

        let mut transaction_signers = cfg_signers.to_local();
        transaction_signers.extend(signers.clone().into_values());
        f(client, group, transaction_signers)
    }

    /// Estimate execution fee.
    pub fn estimate_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        self.group.estimate_execution_fee(
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
        )
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
        before_sign: impl FnMut(&VersionedMessage) -> crate::Result<()>,
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
            .or(Some(self.ctx.client.commitment().commitment));

        let Self {
            ctx:
                Ctx {
                    client,
                    cfg_signers,
                    signers,
                },
            group,
        } = self;

        let latest_hash = client
            .get_latest_blockhash()
            .await
            .map_err(|err| (vec![], Box::new(err).into()))?;

        let mut transaction_signers = cfg_signers.to_local();
        transaction_signers.extend(signers.into_values());

        let txns = group
            .to_transactions_with_options(
                &transaction_signers,
                latest_hash,
                false,
                ComputeBudgetOptions {
                    without_compute_budget,
                    compute_unit_price_micro_lamports,
                    compute_unit_min_priority_lamports,
                },
                before_sign,
            )
            .collect::<crate::Result<Vec<_>>>()
            .map_err(|err| (vec![], err))?;
        send_all_txns(
            &client,
            txns,
            config,
            continue_on_error,
            !disable_error_tracing,
            inspector_cluster,
        )
        .await
    }
}

async fn send_all_txns(
    client: &RpcClient,
    txns: Vec<Vec<VersionedTransaction>>,
    config: RpcSendTransactionConfig,
    continue_on_error: bool,
    enable_tracing: bool,
    inspector_cluster: Option<Cluster>,
) -> Result<Vec<WithSlot<Signature>>, (Vec<WithSlot<Signature>>, crate::Error)> {
    let size = txns.iter().map(|txns| txns.len()).sum();
    let mut signatures = Vec::with_capacity(size);
    let mut error = None;
    for (batch_idx, txns) in txns.into_iter().enumerate() {
        let mut batch = txns
            .iter().enumerate()
            .map(|(idx, txn)| {
                tracing::debug!(
                    %batch_idx,
                    commitment = ?client.commitment(),
                    ?config,
                    "sending transaction {idx}"
                );
                let inspector_cluster = inspector_cluster.clone();
                client
                    .send_and_confirm_transaction_with_config(txn, config)
                    .then(move |res| match res {
                        Ok(signature) => {
                            std::future::ready(Ok(signature))
                        }
                        Err(err) => {
                            if enable_tracing {
                                let cluster = inspector_cluster
                                    .clone()
                                    .or_else(|| client.url().parse().ok());
                                let inspector_url =
                                    inspect_transaction(&txn.message, cluster.as_ref(), false);
                                let hash = txn.message.recent_blockhash();
                                tracing::error!(%err, %hash, ?config, "[batch {batch_idx}] transaction {idx} failed: {inspector_url}");
                            }
                            std::future::ready(Err(err))
                        }
                    })
            })
            .collect::<FuturesOrdered<_>>();
        while let Some(res) = batch.next().await {
            match res {
                Ok(signature) => signatures.push(signature),
                Err(err) => {
                    error = Some(Box::new(err).into());
                    if !continue_on_error {
                        break;
                    }
                }
            }
        }
    }
    match error {
        None => Ok(signatures),
        Some(err) => Err((signatures, err)),
    }
}
