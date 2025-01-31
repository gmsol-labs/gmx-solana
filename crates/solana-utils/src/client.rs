use std::time::Duration;

use solana_client::{
    client_error::ClientError as SolanaClientError, nonblocking::rpc_client::RpcClient,
    rpc_client::SerializableTransaction, rpc_config::RpcSendTransactionConfig,
    rpc_request::RpcError,
};
use solana_sdk::{commitment_config::CommitmentConfig, signature::Signature};
use tokio::time::sleep;

pub(crate) trait SendAndConfirm {
    async fn send_and_confirm_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> std::result::Result<Signature, SolanaClientError>;
}

impl SendAndConfirm for RpcClient {
    async fn send_and_confirm_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> std::result::Result<Signature, SolanaClientError> {
        const SEND_RETRIES: usize = 1;
        const GET_STATUS_RETRIES: usize = usize::MAX;

        'sending: for _ in 0..SEND_RETRIES {
            let signature = self
                .send_transaction_with_config(transaction, config)
                .await?;

            let recent_blockhash = if transaction.uses_durable_nonce() {
                let (recent_blockhash, ..) = self
                    .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
                    .await?;
                recent_blockhash
            } else {
                *transaction.get_recent_blockhash()
            };

            for status_retry in 0..GET_STATUS_RETRIES {
                match self.get_signature_status(&signature).await? {
                    Some(Ok(_)) => return Ok(signature),
                    Some(Err(e)) => return Err(e.into()),
                    None => {
                        if !self
                            .is_blockhash_valid(&recent_blockhash, CommitmentConfig::processed())
                            .await?
                        {
                            // Block hash is not found by some reason
                            break 'sending;
                        } else if cfg!(not(test))
                            // Ignore sleep at last step.
                            && status_retry < GET_STATUS_RETRIES
                        {
                            // Retry twice a second
                            sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                    }
                }
            }
        }

        Err(RpcError::ForUser(
            "unable to confirm transaction. \
             This can happen in situations such as transaction expiration \
             and insufficient fee-payer funds"
                .to_string(),
        )
        .into())
    }
}
