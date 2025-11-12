#![allow(clippy::result_large_err)]

#[cfg(all(feature = "jito", client))]
use futures_util::stream::{self, StreamExt};

#[cfg(all(feature = "jito", client))]
use solana_sdk::{
    hash::Hash, message::VersionedMessage, signer::Signer, transaction::VersionedTransaction,
};

#[cfg(all(feature = "jito", client))]
use base64::Engine;

#[cfg(all(feature = "jito", client))]
use crate::{
    instruction_group::ComputeBudgetOptions, transaction_builder::default_before_sign,
    transaction_group::TransactionGroup,
};

#[cfg(all(feature = "jito", client))]
/// Packing strategy for Jito bundles.
#[derive(Debug, Clone)]
pub enum BundleMode {
    /// One transaction per bundle.
    SingleTx,
    /// Pack multiple transactions into the same bundle within a single ParallelGroup,
    /// respecting `is_mergeable` flags and `max_txs_per_bundle`.
    PackWithinPG {
        /// Maximum number of transactions per bundle.
        max_txs_per_bundle: usize,
    },
    /// Pack across adjacent mergeable ParallelGroups, as long as all AGs are mergeable
    /// and the total count per bundle does not exceed the limit.
    PackAcrossMergeablePGs {
        /// Maximum number of transactions per bundle.
        max_txs_per_bundle: usize,
    },
}

#[cfg(all(feature = "jito", client))]
impl Default for BundleMode {
    fn default() -> Self {
        // Prefer packing as much as possible while respecting AG/PG mergeable semantics.
        Self::PackAcrossMergeablePGs {
            max_txs_per_bundle: 5,
        }
    }
}

#[cfg(all(feature = "jito", client))]
/// Options for sending Jito bundles.
#[derive(Debug, Clone, Default)]
pub struct JitoSendOptions {
    /// Whether to omit injecting compute budget instructions.
    pub without_compute_budget: bool,
    /// Compute unit price in micro-lamports.
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Minimum priority fee in lamports.
    pub compute_unit_min_priority_lamports: Option<u64>,
    /// Whether to continue submitting bundles within a batch when an error occurs.
    pub continue_on_error: bool,
    /// Jito Block Engine base url, e.g. https://ny.mainnet.block-engine.jito.wtf
    pub endpoint_url: String,
    /// Optional UUID query param attached by SDK (see scripts/jito-rust-rpc-master/src/lib.rs)
    pub uuid: Option<String>,
    /// Bundle packing strategy.
    pub bundle_mode: BundleMode,
    /// Optional concurrency for bundle submissions. If `None`, submissions are sequential.
    pub parallelism: Option<usize>,
}

#[cfg(all(feature = "jito", client))]
/// A Jito sending group built on `TransactionGroup` that encodes parallel/serial bundle topology.
#[derive(Debug, Clone)]
pub struct JitoGroup {
    inner: TransactionGroup,
}

#[cfg(all(feature = "jito", client))]
impl From<TransactionGroup> for JitoGroup {
    fn from(inner: TransactionGroup) -> Self {
        Self { inner }
    }
}

#[cfg(all(feature = "jito", client))]
#[cfg(feature = "client")]
impl<'a, C> From<crate::bundle_builder::Bundle<'a, C>> for JitoGroup
where
    C: std::ops::Deref + Clone,
    C::Target: Signer + Sized,
{
    fn from(bundle: crate::bundle_builder::Bundle<'a, C>) -> Self {
        Self {
            inner: bundle.into_group(),
        }
    }
}

#[cfg(all(feature = "jito", client))]
impl JitoGroup {
    /// Returns the inner `TransactionGroup`.
    pub fn into_inner(self) -> TransactionGroup {
        self.inner
    }

    /// Sends bundles according to the `TransactionGroup` parallel/serial semantics,
    /// mapping each transaction to a single-transaction bundle and submitting to the Jito endpoint.
    ///
    /// - Batches are processed sequentially; transactions within a batch are submitted concurrently.
    /// - Returns bundle ids (or raw JSON text) for successful submissions.
    pub async fn send_with_options(
        &self,
        signers: &crate::signer::TransactionSigners<impl std::ops::Deref<Target = dyn Signer>>,
        recent_blockhash: Hash,
        opts: JitoSendOptions,
    ) -> Result<Vec<String>, (Vec<String>, crate::Error)> {
        // Delegate to detailed API and aggregate per legacy signature.
        let cont = opts.continue_on_error;
        let detailed = self
            .send_detailed_with_options(signers, recent_blockhash, opts)
            .await;
        let mut ok_ids: Vec<String> = Vec::new();
        let mut errs: Vec<crate::Error> = Vec::new();
        for res in detailed {
            match res {
                Ok(id) => ok_ids.push(id),
                Err(err) => errs.push(err),
            }
        }
        if errs.is_empty() {
            return Ok(ok_ids);
        }
        // If only one error and not continuing, return first error with partial ids for backward compatibility.
        if !cont && errs.len() == 1 {
            return Err((ok_ids, errs.into_iter().next().unwrap()));
        }
        // Aggregate all errors into a single custom message.
        let preview = errs
            .iter()
            .map(|e| e.to_string())
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        let msg = if errs.len() <= 3 {
            format!("{} bundle(s) failed: {}", errs.len(), preview)
        } else {
            format!("{} bundle(s) failed: {} ...", errs.len(), preview)
        };
        Err((ok_ids, crate::Error::custom(msg)))
    }

    /// Submit bundles and return per-bundle results in order.
    pub async fn send_detailed_with_options(
        &self,
        signers: &crate::signer::TransactionSigners<impl std::ops::Deref<Target = dyn Signer>>,
        recent_blockhash: Hash,
        opts: JitoSendOptions,
    ) -> Vec<Result<String, crate::Error>> {
        let plan = match self
            .build_bundle_plan(signers, recent_blockhash, &opts)
            .await
        {
            Ok(p) => p,
            Err(e) => return vec![Err(e)],
        };

        self.submit_bundles(
            &plan,
            &opts.endpoint_url,
            opts.uuid.as_deref(),
            opts.parallelism,
        )
        .await
    }
}

#[cfg(all(feature = "jito", client))]
fn encode_txn_base64(txn: &VersionedTransaction) -> crate::Result<String> {
    let bytes = bincode::serialize(txn).map_err(|e| crate::Error::custom(e.to_string()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(all(feature = "jito", client))]
impl JitoGroup {
    /// Build a bundle plan from the underlying TransactionGroup according to the bundle_mode.
    /// This function constructs the logical grouping of transactions into bundles without performing network I/O.
    pub async fn build_bundle_plan(
        &self,
        signers: &crate::signer::TransactionSigners<impl std::ops::Deref<Target = dyn Signer>>,
        recent_blockhash: Hash,
        opts: &JitoSendOptions,
    ) -> crate::Result<Vec<Vec<VersionedTransaction>>> {
        let compute_budget = ComputeBudgetOptions {
            without_compute_budget: opts.without_compute_budget,
            compute_unit_price_micro_lamports: opts.compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports: opts.compute_unit_min_priority_lamports,
        };

        // Build transactions per ParallelGroup (batch) in order.
        let batches = self
            .inner
            .to_transactions_with_options(
                signers,
                recent_blockhash,
                false,
                compute_budget,
                default_before_sign as fn(&VersionedMessage) -> crate::Result<()>,
            )
            .collect::<crate::Result<Vec<_>>>()?;

        // Gather PG metadata for packing decisions.
        let pgs = self.inner.groups();
        if pgs.len() != batches.len() {
            return Err(crate::Error::custom("mismatched PG and batch lengths"));
        }

        struct PgTx {
            pg_mergeable: bool,
            ag_mergeable: Vec<bool>,
            txns: Vec<VersionedTransaction>,
        }

        let mut pg_txs: Vec<PgTx> = Vec::with_capacity(batches.len());
        for (pg, txs) in pgs.iter().zip(batches.into_iter()) {
            let ag_flags: Vec<bool> = pg.iter().map(|ag| ag.is_mergeable()).collect();
            if ag_flags.len() != txs.len() {
                return Err(crate::Error::custom("mismatched AG and tx counts in PG"));
            }
            pg_txs.push(PgTx {
                pg_mergeable: pg.is_mergeable(),
                ag_mergeable: ag_flags,
                txns: txs,
            });
        }

        let max_default = 5usize;
        let mut plan: Vec<Vec<VersionedTransaction>> = Vec::new();

        match opts.bundle_mode.clone() {
            BundleMode::SingleTx => {
                for p in pg_txs.into_iter() {
                    for tx in p.txns.into_iter() {
                        plan.push(vec![tx]);
                    }
                }
            }
            BundleMode::PackWithinPG { max_txs_per_bundle } => {
                let limit = if max_txs_per_bundle == 0 {
                    max_default
                } else {
                    max_txs_per_bundle
                };
                for mut p in pg_txs.into_iter() {
                    let mut cur: Vec<VersionedTransaction> = Vec::new();
                    for (tx, ag_ok) in p.txns.drain(..).zip(p.ag_mergeable.into_iter()) {
                        if ag_ok {
                            cur.push(tx);
                            if cur.len() >= limit {
                                plan.push(std::mem::take(&mut cur));
                            }
                        } else {
                            if !cur.is_empty() {
                                plan.push(std::mem::take(&mut cur));
                            }
                            plan.push(vec![tx]);
                        }
                    }
                    if !cur.is_empty() {
                        plan.push(cur);
                    }
                }
            }
            BundleMode::PackAcrossMergeablePGs { max_txs_per_bundle } => {
                let limit = if max_txs_per_bundle == 0 {
                    max_default
                } else {
                    max_txs_per_bundle
                };
                let mut cur: Vec<VersionedTransaction> = Vec::new();
                let mut cur_open = false;
                for p in pg_txs.into_iter() {
                    if !p.pg_mergeable {
                        if !cur.is_empty() {
                            plan.push(std::mem::take(&mut cur));
                        }
                        cur_open = false;
                        for tx in p.txns.into_iter() {
                            plan.push(vec![tx]);
                        }
                        continue;
                    }
                    for (tx, ag_ok) in p.txns.into_iter().zip(p.ag_mergeable.into_iter()) {
                        if ag_ok {
                            if !cur_open {
                                cur_open = true;
                            }
                            cur.push(tx);
                            if cur.len() >= limit {
                                plan.push(std::mem::take(&mut cur));
                                cur_open = false;
                            }
                        } else {
                            if !cur.is_empty() {
                                plan.push(std::mem::take(&mut cur));
                            }
                            cur_open = false;
                            plan.push(vec![tx]);
                        }
                    }
                }
                if !cur.is_empty() {
                    plan.push(cur);
                }
            }
        }

        Ok(plan)
    }

    /// Submit a bundle plan to the Jito endpoint.
    /// By default, submissions are sequential; if `parallelism` is `Some(n)` and `n > 1`,
    /// bounded concurrency is used while preserving result order.
    pub async fn submit_bundles(
        &self,
        plan: &[Vec<VersionedTransaction>],
        endpoint_url: &str,
        uuid: Option<&str>,
        parallelism: Option<usize>,
    ) -> Vec<Result<String, crate::Error>> {
        if plan.is_empty() {
            return Vec::new();
        }

        let limit = parallelism.unwrap_or(1);
        if limit <= 1 {
            // Sequential path
            let mut results = Vec::with_capacity(plan.len());
            for bundle in plan.iter() {
                let mut tx_strings = Vec::with_capacity(bundle.len());
                for tx in bundle.iter() {
                    match encode_txn_base64(tx) {
                        Ok(s) => tx_strings.push(serde_json::Value::String(s)),
                        Err(e) => {
                            results.push(Err(e));
                            continue;
                        }
                    }
                }
                if tx_strings.is_empty() {
                    results.push(Err(crate::Error::custom("empty bundle after encoding")));
                    continue;
                }
                let params = serde_json::Value::Array(tx_strings);
                let res =
                    jito_sdk_rust::JitoJsonRpcSDK::new(endpoint_url, uuid.map(|s| s.to_string()))
                        .send_bundle(Some(params), uuid)
                        .await
                        .map_err(|e| crate::Error::custom(e.to_string()))
                        .and_then(|value| {
                            Ok(value
                                .get("result")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| value.to_string()))
                        });
                results.push(res);
            }
            return results;
        }

        // Concurrent path with ordered aggregation
        let total = plan.len();
        let jobs: Vec<(usize, Vec<VersionedTransaction>)> =
            plan.iter().cloned().enumerate().collect();

        let mut ordered: Vec<Option<Result<String, crate::Error>>> =
            (0..total).map(|_| None).collect();
        let mut s = stream::iter(jobs.into_iter().map(|(idx, bundle)| {
            let endpoint_url = endpoint_url.to_string();
            let uuid = uuid.map(|s| s.to_string());
            async move {
                let mut tx_strings = Vec::with_capacity(bundle.len());
                for tx in bundle.into_iter() {
                    match encode_txn_base64(&tx) {
                        Ok(s) => tx_strings.push(serde_json::Value::String(s)),
                        Err(e) => return (idx, Err(e)),
                    }
                }
                if tx_strings.is_empty() {
                    return (
                        idx,
                        Err(crate::Error::custom("empty bundle after encoding")),
                    );
                }
                let params = serde_json::Value::Array(tx_strings);
                let uuid_ref = uuid.as_deref();
                let uuid_for_new = uuid.clone();
                let res = jito_sdk_rust::JitoJsonRpcSDK::new(&endpoint_url, uuid_for_new)
                    .send_bundle(Some(params), uuid_ref)
                    .await
                    .map_err(|e| crate::Error::custom(e.to_string()))
                    .and_then(|value| {
                        Ok(value
                            .get("result")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| value.to_string()))
                    });
                (idx, res)
            }
        }))
        .buffer_unordered(limit);

        while let Some((idx, res)) = s.next().await {
            ordered[idx] = Some(res);
        }

        ordered.into_iter().map(|x| x.unwrap()).collect()
    }
}
