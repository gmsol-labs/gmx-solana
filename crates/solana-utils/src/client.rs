use std::{future::Future, time::Duration};

use solana_client::{
    client_error::ClientError as SolanaClientError,
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
    /// Errors split by whether the transaction reached the cluster:
    /// - [`crate::Error::Client`]: rejected before submission (e.g. preflight), or landed and
    ///   failed on-chain. Either way the outcome is final.
    /// - [`crate::Error::SendUnresolved`]: submitted, but confirmation failed. The outcome is
    ///   unknown and must be looked up by signature before retrying.
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
            .map_err(Box::new)?;

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
    use std::collections::HashMap;

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
}
