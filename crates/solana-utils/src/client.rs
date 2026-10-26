use std::{future::Future, time::Duration};

use solana_client::{
    client_error::{ClientError as SolanaClientError, ClientErrorKind},
    nonblocking::rpc_client::RpcClient,
    rpc_client::SerializableTransaction,
    rpc_config::RpcSendTransactionConfig,
    rpc_request::{RpcError, RpcRequest},
    rpc_response::Response,
};
use solana_sdk::{
    commitment_config::CommitmentConfig, signature::Signature, transaction::TransactionError,
};
use solana_transaction_status::TransactionStatus;
use tokio::time::sleep;

use crate::utils::WithSlot;

/// Add `send_and_confirm_transaction_with_config` method.
pub trait SendAndConfirm {
    /// Send and confirm a transaction.
    fn send_and_confirm_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> impl Future<Output = std::result::Result<WithSlot<Signature>, SolanaClientError>>;

    /// Send and confirm a transaction, keeping the signature when the outcome is unknown.
    ///
    /// Errors split by whether the transaction may have reached the cluster:
    /// - [`crate::Error::Client`]: the RPC node rejected it (e.g. preflight failure), or it
    ///   landed and failed on-chain. Either way the outcome is final.
    /// - [`crate::Error::SendUnresolved`]: anything else (send timed out, connection dropped,
    ///   bad signature in the send response, or confirmation failed). The node may have
    ///   accepted it, so the outcome must be looked up by signature before retrying.
    ///
    /// The default implementation cannot tell these apart and returns every error as
    /// [`crate::Error::Client`]; [`RpcClient`] overrides it.
    fn send_and_confirm_transaction_detailed(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<Signature>>> {
        async move {
            self.send_and_confirm_transaction_with_config(transaction, config)
                .await
                .map_err(|err| Box::new(err).into())
        }
    }
}

impl SendAndConfirm for RpcClient {
    async fn send_and_confirm_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> std::result::Result<WithSlot<Signature>, SolanaClientError> {
        // Same errors as before `SendUnresolved` existed: unwrap it back to its source.
        self.send_and_confirm_transaction_detailed(transaction, config)
            .await
            .map_err(|err| match err {
                crate::Error::Client(err) => *err,
                crate::Error::SendUnresolved { source, .. } => *source,
                err => RpcError::ForUser(err.to_string()).into(),
            })
    }

    async fn send_and_confirm_transaction_detailed(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> crate::Result<WithSlot<Signature>> {
        let signature = self
            .send_transaction_with_config(transaction, config)
            .await
            .map_err(|err| send_error(transaction, err))?;

        match confirm_transaction(self, &signature, transaction).await {
            Ok(Confirmation::Landed { slot }) => Ok(WithSlot::new(slot, signature)),
            Ok(Confirmation::FailedOnChain(err)) => {
                Err(Box::new(SolanaClientError::from(err)).into())
            }
            Err(err) => Err(crate::Error::SendUnresolved {
                signature,
                source: Box::new(err),
            }),
        }
    }
}

/// Classify a failed `sendTransaction` call.
///
/// Only a JSON-RPC error response (e.g. preflight failure, node unhealthy) proves the node
/// rejected the transaction. Any other failure (timeout, dropped connection, malformed or
/// mismatched signature in the response) can happen after the node accepted and forwarded it.
/// The signature is computed locally at signing time, so it is known even without a response.
fn send_error(transaction: &impl SerializableTransaction, err: SolanaClientError) -> crate::Error {
    // Checked before touching the signature, so a rejected unsigned transaction cannot panic.
    let rejected = matches!(
        err.kind(),
        ClientErrorKind::RpcError(RpcError::RpcResponseError { .. })
    );
    match first_signature(transaction) {
        Some(signature) if !rejected => crate::Error::SendUnresolved {
            signature,
            source: Box::new(err),
        },
        // Rejected by the node, or unsigned and so impossible to execute on-chain.
        _ => Box::new(err).into(),
    }
}

/// First signature of `transaction`, or `None` if it has none.
///
/// `get_signature()` panics on a transaction with no signatures, so check the wire format
/// first: it starts with the signature count (compact-u16), which is a single `0` byte when
/// there are none.
fn first_signature(transaction: &impl SerializableTransaction) -> Option<Signature> {
    let bytes = bincode::serialize(transaction).ok()?;
    if bytes.first() == Some(&0) {
        return None;
    }
    Some(*transaction.get_signature())
}

/// Pause between block height polls in [`wait_for_blockhash_expiry`].
const EXPIRY_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Polls before [`wait_for_blockhash_expiry`] gives up: ~3 min at 500 ms, about twice a
/// blockhash lifetime (150 blocks, ~60–90 s on mainnet).
const EXPIRY_MAX_POLLS: usize = 360;
/// RPC errors in a row that [`wait_for_blockhash_expiry`] tolerates before failing.
const EXPIRY_MAX_RPC_ERRORS: usize = 5;

/// Wait until a transaction whose blockhash is valid up to `last_valid_block_height` can no
/// longer land.
///
/// Polls the block height at `commitment` every 500 ms and returns once it is above
/// `last_valid_block_height`, so it returns at once when the blockhash already expired.
/// A lagging RPC node reports a lower height, which can only make the wait longer, never end
/// it early. Use it to settle [`crate::Error::SendUnresolved`] before reading the state the
/// transaction would have changed. `last_valid_block_height` comes with the blockhash from
/// [`RpcClient::get_latest_blockhash_with_commitment`].
///
/// Pass the same `commitment` the later reads use. Once the height at that level is past
/// `last_valid_block_height`, every block that could hold the transaction is at that level
/// too, so the reads see it. A stricter read (e.g. `finalized` after a `confirmed` wait) can
/// lag by seconds and miss a transaction that did land.
///
/// Fails after 5 RPC errors in a row, or when the height has not passed after ~3 minutes.
/// Durable-nonce transactions do not expire with their blockhash and are not covered.
pub async fn wait_for_blockhash_expiry(
    client: &RpcClient,
    last_valid_block_height: u64,
    commitment: CommitmentConfig,
) -> crate::Result<()> {
    let mut rpc_errors = 0;
    for _ in 0..EXPIRY_MAX_POLLS {
        match client.get_block_height_with_commitment(commitment).await {
            Ok(height) if height > last_valid_block_height => return Ok(()),
            Ok(_) => rpc_errors = 0,
            Err(err) => {
                rpc_errors += 1;
                if rpc_errors >= EXPIRY_MAX_RPC_ERRORS {
                    return Err(Box::new(err).into());
                }
            }
        }
        if cfg!(not(test)) {
            sleep(EXPIRY_POLL_INTERVAL).await;
        }
    }
    Err(crate::Error::custom(format!(
        "block height did not pass {last_valid_block_height} in time"
    )))
}

/// Final status of a submitted transaction.
enum Confirmation {
    Landed { slot: u64 },
    FailedOnChain(TransactionError),
}

/// Poll the status of an already-submitted transaction until it reaches the client's
/// commitment, or until its blockhash expires (returned as an error).
async fn confirm_transaction(
    client: &RpcClient,
    signature: &Signature,
    transaction: &impl SerializableTransaction,
) -> Result<Confirmation, SolanaClientError> {
    let recent_blockhash = if transaction.uses_durable_nonce() {
        let (recent_blockhash, ..) = client
            .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
            .await?;
        recent_blockhash
    } else {
        *transaction.get_recent_blockhash()
    };

    loop {
        let result: Response<Vec<Option<TransactionStatus>>> = client
            .send(
                RpcRequest::GetSignatureStatuses,
                serde_json::json!([[signature.to_string()]]),
            )
            .await?;
        let status = result.value[0]
            .clone()
            .filter(|status| status.satisfies_commitment(client.commitment()));

        if let Some(status) = status {
            return Ok(match status.status {
                Ok(()) => Confirmation::Landed { slot: status.slot },
                Err(err) => Confirmation::FailedOnChain(err),
            });
        }

        // An expired blockhash means the transaction almost certainly did not land, but this
        // is not provable, so the caller still gets an "unresolved" error, not a failure.
        if !client
            .is_blockhash_valid(&recent_blockhash, CommitmentConfig::processed())
            .await?
        {
            return Err(RpcError::ForUser(
                "unable to confirm transaction. \
                 This can happen in situations such as transaction expiration \
                 and insufficient fee-payer funds"
                    .to_string(),
            )
            .into());
        }

        if cfg!(not(test)) {
            // Retry twice a second
            sleep(Duration::from_millis(500)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        future::Future,
        io,
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use solana_client::{
        client_error::Result as ClientResult,
        rpc_client::RpcClientConfig,
        rpc_request::RpcResponseErrorData,
        rpc_sender::{RpcSender, RpcTransportStats},
    };

    use solana_sdk::{
        hash::Hash, signature::Keypair, signer::Signer, system_instruction,
        transaction::Transaction,
    };

    use super::*;

    fn signed_transaction() -> Transaction {
        let payer = Keypair::new();
        let ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
        Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer],
            Hash::new_unique(),
        )
    }

    fn skip_preflight() -> RpcSendTransactionConfig {
        RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        }
    }

    fn blockhash_valid(valid: bool) -> serde_json::Value {
        serde_json::json!({ "context": { "slot": 1 }, "value": valid })
    }

    /// RPC transport whose every request fails with the given error.
    struct FailingSender(fn() -> SolanaClientError);

    // Written out by hand because `RpcSender` is an `#[async_trait]` trait.
    impl RpcSender for FailingSender {
        fn send<'a, 'b>(
            &'a self,
            _request: RpcRequest,
            _params: serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'b>>
        where
            'a: 'b,
            Self: 'b,
        {
            let err = (self.0)();
            Box::pin(async move { Err(err) })
        }

        fn get_transport_stats(&self) -> RpcTransportStats {
            RpcTransportStats::default()
        }

        fn url(&self) -> String {
            "failing".to_string()
        }
    }

    async fn send_with_failing_transport(
        make_error: fn() -> SolanaClientError,
    ) -> (Transaction, crate::Error) {
        let client = RpcClient::new_sender(FailingSender(make_error), RpcClientConfig::default());
        let tx = signed_transaction();
        let err = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();
        (tx, err)
    }

    fn rejected_by_node() -> SolanaClientError {
        RpcError::RpcResponseError {
            code: -32002,
            message: "Transaction simulation failed".to_string(),
            data: RpcResponseErrorData::Empty,
        }
        .into()
    }

    fn timed_out() -> SolanaClientError {
        io::Error::new(io::ErrorKind::TimedOut, "timed out").into()
    }

    fn unsigned_transaction() -> Transaction {
        let payer = Keypair::new();
        let ix = system_instruction::transfer(&payer.pubkey(), &payer.pubkey(), 1);
        let tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
        Transaction {
            signatures: vec![],
            ..tx
        }
    }

    #[tokio::test]
    async fn rejected_unsigned_transaction_is_an_error_not_a_panic() {
        let client = RpcClient::new_sender(FailingSender(rejected_by_node), Default::default());
        let tx = unsigned_transaction();

        let detailed = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();
        assert!(matches!(detailed, crate::Error::Client(_)), "{detailed:?}");

        let legacy = client
            .send_and_confirm_transaction_with_config(&tx, skip_preflight())
            .await
            .unwrap_err();
        assert!(
            legacy.to_string().contains("Transaction simulation failed"),
            "{legacy}"
        );
    }

    #[tokio::test]
    async fn timed_out_unsigned_transaction_is_an_error_not_a_panic() {
        let client = RpcClient::new_sender(FailingSender(timed_out), Default::default());

        let err = client
            .send_and_confirm_transaction_detailed(&unsigned_transaction(), skip_preflight())
            .await
            .unwrap_err();

        assert!(matches!(err, crate::Error::Client(_)), "{err:?}");
    }

    #[tokio::test]
    async fn send_timeout_is_unresolved() {
        let (tx, err) = send_with_failing_transport(timed_out).await;
        assert_eq!(err.unresolved_signature(), Some(&tx.signatures[0]));
    }

    #[tokio::test]
    async fn node_rejection_is_a_client_error() {
        let (_, err) = send_with_failing_transport(rejected_by_node).await;
        assert!(matches!(err, crate::Error::Client(_)), "{err:?}");
    }

    #[tokio::test]
    async fn malformed_send_response_is_unresolved() {
        let mocks = HashMap::from([(
            RpcRequest::SendTransaction,
            serde_json::json!("not-a-signature"),
        )]);
        let client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
        let tx = signed_transaction();

        let err = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();

        assert_eq!(err.unresolved_signature(), Some(&tx.signatures[0]));
    }

    #[tokio::test]
    async fn mismatched_send_response_keeps_the_local_signature() {
        // "malicious" makes the mock return a signature that is not the transaction's.
        let client = RpcClient::new_mock("malicious".to_string());
        let tx = signed_transaction();

        let err = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();

        assert_eq!(err.unresolved_signature(), Some(&tx.signatures[0]));
    }

    #[tokio::test]
    async fn landed_transaction_returns_signature_and_slot() {
        let client = RpcClient::new_mock("succeeds".to_string());
        let tx = signed_transaction();

        let sent = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap();

        assert_eq!(sent.slot(), 1);
        assert_eq!(*sent.value(), tx.signatures[0]);
    }

    #[tokio::test]
    async fn old_method_still_returns_the_client_error_when_unresolved() {
        let mocks = HashMap::from([(RpcRequest::IsBlockhashValid, blockhash_valid(false))]);
        let client = RpcClient::new_mock_with_mocks("sig_not_found".to_string(), mocks);

        let err = client
            .send_and_confirm_transaction_with_config(&signed_transaction(), skip_preflight())
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("unable to confirm transaction"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn on_chain_failure_is_a_client_error() {
        let client = RpcClient::new_mock("instruction_error".to_string());

        let err = client
            .send_and_confirm_transaction_detailed(&signed_transaction(), skip_preflight())
            .await
            .unwrap_err();

        assert!(matches!(err, crate::Error::Client(_)), "{err:?}");
    }

    #[tokio::test]
    async fn expired_blockhash_after_submit_is_unresolved() {
        let mocks = HashMap::from([(RpcRequest::IsBlockhashValid, blockhash_valid(false))]);
        let client = RpcClient::new_mock_with_mocks("sig_not_found".to_string(), mocks);
        let tx = signed_transaction();

        let err = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();

        assert_eq!(err.unresolved_signature(), Some(&tx.signatures[0]));
    }

    #[tokio::test]
    async fn rpc_error_while_polling_is_unresolved() {
        // "fails" makes every un-mocked request return `null`, so only `sendTransaction`
        // succeeds and the status poll fails to deserialize.
        let tx = signed_transaction();
        let mocks = HashMap::from([(
            RpcRequest::SendTransaction,
            serde_json::json!(tx.signatures[0].to_string()),
        )]);
        let client = RpcClient::new_mock_with_mocks("fails".to_string(), mocks);

        let err = client
            .send_and_confirm_transaction_detailed(&tx, skip_preflight())
            .await
            .unwrap_err();

        assert_eq!(err.unresolved_signature(), Some(&tx.signatures[0]));
    }

    /// Answers `getBlockHeight` from a script, one entry per call: `Some(height)`, or `None`
    /// for an RPC error. The last entry repeats once the script runs out. Every request
    /// must carry `expected_commitment`.
    struct BlockHeights {
        script: Vec<Option<u64>>,
        polls: Arc<AtomicUsize>,
        expected_commitment: &'static str,
    }

    impl RpcSender for BlockHeights {
        fn send<'a, 'b>(
            &'a self,
            request: RpcRequest,
            params: serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'b>>
        where
            'a: 'b,
            Self: 'b,
        {
            assert_eq!(request, RpcRequest::GetBlockHeight);
            assert_eq!(
                params[0]["commitment"], self.expected_commitment,
                "{params}"
            );
            let poll = self.polls.fetch_add(1, Ordering::SeqCst);
            let answer = self.script[poll.min(self.script.len() - 1)];
            Box::pin(async move {
                match answer {
                    Some(height) => Ok(serde_json::json!(height)),
                    None => Err(timed_out()),
                }
            })
        }

        fn get_transport_stats(&self) -> RpcTransportStats {
            RpcTransportStats::default()
        }

        fn url(&self) -> String {
            "block-heights".to_string()
        }
    }

    /// Runs the wait with `last_valid_block_height = 150` at `confirmed` and returns its
    /// result and poll count.
    async fn wait_with_heights(script: Vec<Option<u64>>) -> (crate::Result<()>, usize) {
        let polls = Arc::new(AtomicUsize::new(0));
        let sender = BlockHeights {
            script,
            polls: polls.clone(),
            expected_commitment: "confirmed",
        };
        let client = RpcClient::new_sender(sender, RpcClientConfig::default());
        let result = wait_for_blockhash_expiry(&client, 150, CommitmentConfig::confirmed()).await;
        (result, polls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn wait_polls_at_the_given_commitment() {
        let sender = BlockHeights {
            script: vec![Some(151)],
            polls: Default::default(),
            expected_commitment: "finalized",
        };
        let client = RpcClient::new_sender(sender, RpcClientConfig::default());

        let result = wait_for_blockhash_expiry(&client, 150, CommitmentConfig::finalized()).await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn wait_returns_at_once_when_already_expired() {
        let (result, polls) = wait_with_heights(vec![Some(151)]).await;
        assert!(result.is_ok());
        assert_eq!(polls, 1);
    }

    #[tokio::test]
    async fn wait_polls_until_height_passes() {
        // 150 is still valid: the blockhash expires only above its last valid height.
        let (result, polls) = wait_with_heights(vec![Some(100), Some(150), Some(151)]).await;
        assert!(result.is_ok());
        assert_eq!(polls, 3);
    }

    #[tokio::test]
    async fn wait_survives_transient_rpc_errors() {
        let script = vec![None, None, None, None, Some(120), None, Some(151)];
        let (result, polls) = wait_with_heights(script).await;
        assert!(result.is_ok());
        assert_eq!(polls, 7);
    }

    #[tokio::test]
    async fn wait_fails_after_five_rpc_errors_in_a_row() {
        let (result, polls) = wait_with_heights(vec![None]).await;
        assert!(matches!(result, Err(crate::Error::Client(_))), "{result:?}");
        assert_eq!(polls, 5);
    }

    #[tokio::test]
    async fn wait_gives_up_when_height_never_passes() {
        let (result, polls) = wait_with_heights(vec![Some(100)]).await;
        assert!(result.is_err());
        assert_eq!(polls, EXPIRY_MAX_POLLS);
    }
}
