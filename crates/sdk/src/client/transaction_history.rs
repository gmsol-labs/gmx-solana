use std::borrow::Borrow;

use async_stream::try_stream;
use futures_util::Stream;
use gmsol_solana_utils::utils::WithSlot;
use solana_client::{
    nonblocking::rpc_client::RpcClient, rpc_client::GetConfirmedSignaturesForAddress2Config,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};

#[cfg(feature = "decode")]
use gmsol_decode::decoder::{CPIEvents, TransactionDecoder};

/// Fetch transaction history for an address.
pub async fn fetch_transaction_history_with_config(
    client: impl Borrow<RpcClient>,
    address: &Pubkey,
    commitment: CommitmentConfig,
    until: Option<Signature>,
    mut before: Option<Signature>,
    batch: Option<usize>,
) -> crate::Result<impl Stream<Item = crate::Result<WithSlot<Signature>>>> {
    let limit = batch;
    let commitment = Some(commitment);
    let address = *address;

    let stream = try_stream! {
        loop {
            let txns = client.borrow().get_signatures_for_address_with_config(&address, GetConfirmedSignaturesForAddress2Config {
                before,
                until,
                limit,
                commitment,
            }).await.map_err(crate::Error::custom)?;
            match txns.last() {
                Some(next) => {
                    let next = next.signature.parse().map_err(crate::Error::custom)?;
                    for txn in txns {
                        let slot = txn.slot;
                        let signature = txn.signature.parse().map_err(crate::Error::custom)?;
                        yield WithSlot::new(slot, signature);
                    }
                    before = Some(next);
                },
                None => {
                    break;
                }
            }
        }
    };
    Ok(stream)
}

/// Whether a fetched transaction failed; `None` when the RPC response has no status.
///
/// A failed transaction is rolled back, but its inner instructions stay in the record, so
/// CPI events emitted before the failure are still there, e.g. a `TradeEvent` from an
/// execution whose bundled close then failed. Those events never happened.
#[cfg(feature = "decode")]
fn transaction_failed(
    meta: Option<&solana_transaction_status::UiTransactionStatusMeta>,
) -> Option<bool> {
    meta.map(|meta| meta.err.is_some())
}

/// Extract encoded CPI events from transaction history.
///
/// Failed transactions are skipped: their events were rolled back. A transaction whose
/// status is missing gives an error, since its events can't be trusted either way.
#[cfg(feature = "decode")]
pub fn extract_cpi_events(
    stream: impl Stream<Item = crate::Result<WithSlot<Signature>>>,
    client: impl Borrow<RpcClient>,
    program_id: &Pubkey,
    event_authority: &Pubkey,
    commitment: CommitmentConfig,
    max_supported_transaction_version: Option<u8>,
) -> impl Stream<Item = crate::Result<WithSlot<CPIEvents>>> {
    use solana_client::rpc_config::RpcTransactionConfig;
    use solana_transaction_status::UiTransactionEncoding;

    let program_id = *program_id;
    let event_authority = *event_authority;
    async_stream::stream! {
        for await res in stream {
            match res {
                Ok(ctx) => {
                    let signature = *ctx.value();
                    tracing::debug!(%signature, "fetching transaction");
                    let tx = client
                        .borrow()
                        .get_transaction_with_config(
                            &signature,
                            RpcTransactionConfig {
                                encoding: Some(UiTransactionEncoding::Base64),
                                commitment: Some(commitment),
                                max_supported_transaction_version,
                            },
                        )
                        .await
                        .map_err(crate::Error::custom)?;
                    match transaction_failed(tx.transaction.meta.as_ref()) {
                        Some(false) => {}
                        Some(true) => {
                            tracing::debug!(%signature, "skipping failed transaction");
                            continue;
                        }
                        None => {
                            yield Err(crate::Error::custom(format!(
                                "transaction {signature} has no status"
                            )));
                            continue;
                        }
                    }
                    let mut decoder = TransactionDecoder::new(tx.slot, signature, &tx.transaction);
                    match decoder
                        .add_cpi_event_authority_and_program_id(event_authority, program_id)
                        .and_then(|decoder| decoder.extract_cpi_events())
                    {
                        Ok(events) => {
                            yield Ok(ctx.map(|_| events));
                        },
                        Err(err) => {
                            yield Err(err.into());
                        }
                    }
                },
                Err(err) => {
                    yield Err(err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;
    use crate::test::{default_cluster, setup_fmt_tracing};

    #[cfg(feature = "decode")]
    #[test]
    fn failed_transactions_are_detected() {
        use solana_sdk::{instruction::InstructionError, transaction::TransactionError};
        use solana_transaction_status::{TransactionStatusMeta, UiTransactionStatusMeta};

        let ok = UiTransactionStatusMeta::from(TransactionStatusMeta::default());
        // Shape seen on devnet: a later instruction (index 3) fails with a custom error.
        let failed = UiTransactionStatusMeta::from(TransactionStatusMeta {
            status: Err(TransactionError::InstructionError(
                3,
                InstructionError::Custom(1),
            )),
            ..Default::default()
        });
        assert_eq!(transaction_failed(Some(&ok)), Some(false));
        assert_eq!(transaction_failed(Some(&failed)), Some(true));
        assert_eq!(transaction_failed(None), None);
    }

    #[tokio::test]
    async fn test_transaction_hisotry_fetching() -> crate::Result<()> {
        let _guard = setup_fmt_tracing("info");
        let cluster = default_cluster();
        let client = RpcClient::new(cluster.url().to_string());
        let stream = fetch_transaction_history_with_config(
            client,
            &gmsol_programs::gmsol_store::ID,
            CommitmentConfig::confirmed(),
            None,
            None,
            Some(5),
        )
        .await?
        .take(5);
        futures_util::pin_mut!(stream);
        while let Some(Ok(signature)) = stream.next().await {
            tracing::info!("{signature:?}");
        }
        Ok(())
    }
}
