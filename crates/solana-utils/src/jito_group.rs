#![allow(clippy::result_large_err)]

#[cfg(all(feature = "jito", client))]
use futures_util::{stream::FuturesOrdered, StreamExt};

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
        let compute_budget = ComputeBudgetOptions {
            without_compute_budget: opts.without_compute_budget,
            compute_unit_price_micro_lamports: opts.compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports: opts.compute_unit_min_priority_lamports,
        };

        let batches = self
            .inner
            .to_transactions_with_options(
                signers,
                recent_blockhash,
                false,
                compute_budget,
                default_before_sign as fn(&VersionedMessage) -> crate::Result<()>,
            )
            .collect::<crate::Result<Vec<_>>>()
            .map_err(|e| (vec![], e))?;

        let mut bundle_ids = Vec::new();
        let mut error: Option<crate::Error> = None;

        for txns in batches.into_iter() {
            let mut futures = txns
                .into_iter()
                .map(|txn| {
                    let base_url = opts.endpoint_url.clone();
                    let uuid = opts.uuid.clone();
                    async move {
                        let encoded = encode_txn_base64(&txn);
                        let params =
                            serde_json::Value::Array(vec![serde_json::Value::String(encoded)]);
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
                    }
                })
                .collect::<FuturesOrdered<_>>();

            while let Some(res) = futures.next().await {
                match res {
                    Ok(id) => bundle_ids.push(id),
                    Err(err) => {
                        error = Some(err);
                        if !opts.continue_on_error {
                            return Err((bundle_ids, error.take().unwrap()));
                        }
                    }
                }
            }
        }

        match error {
            None => Ok(bundle_ids),
            Some(err) => Err((bundle_ids, err)),
        }
    }
}

#[cfg(all(feature = "jito", client))]
fn encode_txn_base64(txn: &VersionedTransaction) -> String {
    base64::engine::general_purpose::STANDARD
        .encode(bincode::serialize(txn).expect("serialize txn"))
}
