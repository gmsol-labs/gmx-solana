#![allow(clippy::result_large_err)]

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
        let compute_budget = ComputeBudgetOptions {
            without_compute_budget: opts.without_compute_budget,
            compute_unit_price_micro_lamports: opts.compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports: opts.compute_unit_min_priority_lamports,
        };

        // Build transactions per ParallelGroup (batch) in order.
        let batches = match self
            .inner
            .to_transactions_with_options(
                signers,
                recent_blockhash,
                false,
                compute_budget,
                default_before_sign as fn(&VersionedMessage) -> crate::Result<()>,
            )
            .collect::<crate::Result<Vec<_>>>()
        {
            Ok(v) => v,
            Err(e) => return vec![Err(e)],
        };

        // Gather PG metadata for packing decisions.
        let pgs = self.inner.groups();
        if pgs.len() != batches.len() {
            return vec![Err(crate::Error::custom("mismatched PG and batch lengths"))];
        }

        #[derive(Clone)]
        struct PgTx<'a> {
            pg_mergeable: bool,
            ag_mergeable: Vec<bool>,
            txns: Vec<&'a VersionedTransaction>,
        }

        let mut pg_txs: Vec<PgTx> = Vec::with_capacity(batches.len());
        for (i, txs) in batches.iter().enumerate() {
            let pg = &pgs[i];
            let ag_flags: Vec<bool> = pg.iter().map(|ag| ag.is_mergeable()).collect();
            if ag_flags.len() != txs.len() {
                return vec![Err(crate::Error::custom(
                    "mismatched AG and tx counts in PG",
                ))];
            }
            pg_txs.push(PgTx {
                pg_mergeable: pg.is_mergeable(),
                ag_mergeable: ag_flags,
                txns: txs.iter().collect(),
            });
        }

        let max_default = 5usize;
        let mut plan: Vec<Vec<&VersionedTransaction>> = Vec::new();

        match opts.bundle_mode.clone() {
            BundleMode::SingleTx => {
                for p in &pg_txs {
                    for tx in &p.txns {
                        plan.push(vec![*tx]);
                    }
                }
            }
            BundleMode::PackWithinPG { max_txs_per_bundle } => {
                let limit = if max_txs_per_bundle == 0 {
                    max_default
                } else {
                    max_txs_per_bundle
                };
                for p in &pg_txs {
                    // Always pack within PG by AG mergeability; non-mergeable AGs are solo bundles.
                    let mut cur: Vec<&VersionedTransaction> = Vec::new();
                    for (tx, ag_ok) in p.txns.iter().zip(p.ag_mergeable.iter().copied()) {
                        if ag_ok {
                            cur.push(*tx);
                            if cur.len() >= limit {
                                plan.push(std::mem::take(&mut cur));
                            }
                        } else {
                            if !cur.is_empty() {
                                plan.push(std::mem::take(&mut cur));
                            }
                            plan.push(vec![*tx]);
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
                let mut cur: Vec<&VersionedTransaction> = Vec::new();
                let mut cur_open = false; // whether we are inside a chain of mergeable PGs
                for p in &pg_txs {
                    if !p.pg_mergeable {
                        // Flush current chain
                        if !cur.is_empty() {
                            plan.push(std::mem::take(&mut cur));
                        }
                        cur_open = false;
                        // Non-mergeable PG -> no packing within
                        for tx in &p.txns {
                            plan.push(vec![*tx]);
                        }
                        continue;
                    }
                    // PG is mergeable: try to pack its mergeable AG txns into current chain
                    for (tx, ag_ok) in p.txns.iter().zip(p.ag_mergeable.iter().copied()) {
                        if ag_ok {
                            if !cur_open {
                                cur_open = true;
                            }
                            cur.push(*tx);
                            if cur.len() >= limit {
                                plan.push(std::mem::take(&mut cur));
                                cur_open = false;
                            }
                        } else {
                            // Non-mergeable AG breaks packing; flush and push solo
                            if !cur.is_empty() {
                                plan.push(std::mem::take(&mut cur));
                            }
                            cur_open = false;
                            plan.push(vec![*tx]);
                        }
                    }
                }
                if !cur.is_empty() {
                    plan.push(cur);
                }
            }
        }

        // Now submit each bundle concurrently per stage (we already flattened PG boundaries in plan),
        // but to preserve ordering we can just submit sequentially or chunk by some parallelism.
        // For simplicity and determinism, submit sequentially; callers can parallelize at higher level if needed.
        let mut results: Vec<Result<String, crate::Error>> = Vec::with_capacity(plan.len());
        for bundle in plan.into_iter() {
            let mut tx_strings = Vec::with_capacity(bundle.len());
            for tx in bundle.into_iter() {
                match encode_txn_base64(tx) {
                    Ok(s) => tx_strings.push(serde_json::Value::String(s)),
                    Err(e) => {
                        results.push(Err(e));
                        continue;
                    }
                }
            }
            if tx_strings.is_empty() {
                continue;
            }

            let base_url = opts.endpoint_url.clone();
            let uuid = opts.uuid.clone();
            let params = serde_json::Value::Array(tx_strings);
            let submit = async move {
                let value = jito_sdk_rust::JitoJsonRpcSDK::new(&base_url, uuid.clone())
                    .send_bundle(Some(params), uuid.as_deref())
                    .await
                    .map_err(|e| crate::Error::custom(e.to_string()))?;
                let id = value
                    .get("result")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| value.to_string());
                Ok::<String, crate::Error>(id)
            };

            match submit.await {
                Ok(id) => results.push(Ok(id)),
                Err(err) => results.push(Err(err)),
            }
        }

        results
    }
}

#[cfg(all(feature = "jito", client))]
fn encode_txn_base64(txn: &VersionedTransaction) -> crate::Result<String> {
    let bytes = bincode::serialize(txn).map_err(|e| crate::Error::custom(e.to_string()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
