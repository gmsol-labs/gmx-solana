use gmsol_programs::anchor_lang::error::ErrorCode;
use gmsol_sdk::{
    ops::{StoreOps, SystemProgramOps},
    Client,
};
use gmsol_solana_utils::signer::SignerRef;
use gmsol_store::CoreError;
use solana_sdk::{
    account::Account, instruction::InstructionError, pubkey::Pubkey, signature::Keypair,
    signer::Signer, system_instruction, system_program, transaction::TransactionError,
};

use super::setup::{current_deployment, Deployment};

struct RentFixture {
    client: Client<SignerRef>,
    receiver: Keypair,
    collector: Pubkey,
    destination: Pubkey,
    minimum: u64,
}

impl RentFixture {
    async fn new(deployment: &Deployment) -> eyre::Result<Self> {
        let client = deployment.funded_client().await?;
        let receiver = Keypair::new();
        let collector = client.find_rent_collector_address(&receiver.pubkey());
        let destination = Pubkey::new_unique();
        let minimum = client
            .rpc()
            .get_minimum_balance_for_rent_exemption(0)
            .await?;
        client.transfer(&destination, minimum)?.send().await?;
        Ok(Self {
            client,
            receiver,
            collector,
            destination,
            minimum,
        })
    }

    async fn account(&self, address: &Pubkey) -> eyre::Result<Option<Account>> {
        Ok(self
            .client
            .rpc()
            .get_account_with_commitment(address, self.client.commitment())
            .await?
            .value)
    }

    async fn fund(&self, amount: u64) -> eyre::Result<()> {
        self.client
            .transfer(&self.collector, amount)?
            .send()
            .await?;
        Ok(())
    }

    async fn claim(&self, destination: &Pubkey) -> eyre::Result<()> {
        // Repeated claims must execute again, rather than reuse a processed signature.
        let blockhash = self.client.rpc().get_latest_blockhash().await?;
        self.client
            .rpc()
            .get_new_latest_blockhash(&blockhash)
            .await?;
        let signature = self
            .client
            .claim_rent(&self.receiver.pubkey(), destination)
            .signer(&self.receiver)
            .send()
            .await?;
        tracing::info!(%signature, collector=%self.collector, %destination, "claimed rent");
        Ok(())
    }

    async fn assert_balances(&self, collector: u64, destination: u64) -> eyre::Result<()> {
        let collector_balance = self.client.rpc().get_balance(&self.collector).await?;
        let destination_balance = self.client.rpc().get_balance(&self.destination).await?;
        tracing::info!(collector_balance, destination_balance, "rent balances");
        assert_eq!(collector_balance, collector);
        assert_eq!(destination_balance, destination);
        Ok(())
    }
}

#[tokio::test]
async fn collector_receives_without_initialization_or_receiver_account() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    assert!(f.account(&f.collector).await?.is_none());
    assert!(f.account(&f.receiver.pubkey()).await?.is_none());
    let ix = system_instruction::transfer(&f.client.payer(), &f.collector, f.minimum);
    assert!(!ix.accounts.iter().any(|m| m.pubkey == f.receiver.pubkey()));
    f.client
        .store_transaction()
        .pre_instruction(ix, false)
        .send()
        .await?;
    let account = f
        .account(&f.collector)
        .await?
        .expect("funded collector missing");
    assert_eq!(account.owner, system_program::ID);
    assert!(account.data.is_empty());
    assert_eq!(account.lamports, f.minimum);
    assert!(f.account(&f.receiver.pubkey()).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn receiver_claims_whole_balance_from_multiple_senders() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    let other = deployment.funded_client().await?;
    let first = f.minimum + 17;
    let second = f.minimum + 29;
    f.fund(first).await?;
    other.transfer(&f.collector, second)?.send().await?;
    f.claim(&f.destination).await?;
    f.assert_balances(0, f.minimum + first + second).await?;
    Ok(())
}

#[tokio::test]
async fn drained_collector_can_receive_and_claim_again() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    f.fund(f.minimum).await?;
    f.claim(&f.destination).await?;
    f.assert_balances(0, 2 * f.minimum).await?;
    f.claim(&f.destination).await?;
    f.assert_balances(0, 2 * f.minimum).await?;
    f.fund(2 * f.minimum).await?;
    f.claim(&f.destination).await?;
    f.assert_balances(0, 4 * f.minimum).await?;
    Ok(())
}

#[tokio::test]
async fn never_funded_collector_claim_is_a_noop() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    assert!(f.account(&f.collector).await?.is_none());
    f.claim(&f.destination).await?;
    f.assert_balances(0, f.minimum).await?;
    assert!(f.account(&f.collector).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn collector_rejects_unauthorized_withdrawals() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    f.fund(f.minimum).await?;
    let mut wrong = f
        .client
        .claim_rent(&f.client.payer(), &f.destination)
        .take_instruction()
        .unwrap();
    wrong.accounts[1].pubkey = f.collector;
    let err = f
        .client
        .store_transaction()
        .pre_instruction(wrong, false)
        .send()
        .await
        .expect_err("wrong receiver accepted");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::ConstraintSeeds.into())
    );
    f.assert_balances(f.minimum, f.minimum).await?;

    let mut unsigned = f
        .client
        .claim_rent(&f.receiver.pubkey(), &f.destination)
        .take_instruction()
        .unwrap();
    unsigned.accounts[0].is_signer = false;
    let err = f
        .client
        .store_transaction()
        .pre_instruction(unsigned, false)
        .send()
        .await
        .expect_err("missing receiver signature accepted");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(ErrorCode::AccountNotSigner.into())
    );
    f.assert_balances(f.minimum, f.minimum).await?;

    let mut direct = system_instruction::transfer(&f.collector, &f.destination, f.minimum);
    direct.accounts[0].is_signer = false;
    let err = f
        .client
        .store_transaction()
        .pre_instruction(direct, false)
        .send()
        .await
        .expect_err("unsigned System withdrawal accepted");
    assert!(
        matches!(err, gmsol_solana_utils::Error::Client(ref e) if matches!(e.get_transaction_error(), Some(TransactionError::InstructionError(_, InstructionError::MissingRequiredSignature))))
    );
    f.assert_balances(f.minimum, f.minimum).await?;
    Ok(())
}

#[tokio::test]
async fn self_claim_rejects_nonempty_collector_but_allows_empty() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    f.fund(f.minimum).await?;
    let err = f
        .client
        .claim_rent(&f.receiver.pubkey(), &f.collector)
        .signer(&f.receiver)
        .send()
        .await
        .expect_err("positive self claim accepted");
    assert_eq!(
        gmsol_sdk::Error::from(err).anchor_error_code(),
        Some(CoreError::InvalidArgument.into())
    );
    f.assert_balances(f.minimum, f.minimum).await?;
    f.claim(&f.destination).await?;
    f.claim(&f.collector).await?;
    f.assert_balances(0, 2 * f.minimum).await?;
    Ok(())
}

#[tokio::test]
async fn rent_receiver_can_receive_its_claim() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    f.fund(f.minimum).await?;
    assert!(f.account(&f.receiver.pubkey()).await?.is_none());
    f.claim(&f.receiver.pubkey()).await?;
    assert_eq!(
        f.client.rpc().get_balance(&f.receiver.pubkey()).await?,
        f.minimum
    );
    f.assert_balances(0, f.minimum).await?;
    Ok(())
}

#[tokio::test]
async fn absent_collector_rejects_funding_below_runtime_rent_minimum() -> eyre::Result<()> {
    let deployment = current_deployment().await?;
    let _guard = deployment.use_accounts().await?;
    let f = RentFixture::new(deployment).await?;
    assert!(f.account(&f.collector).await?.is_none());
    assert!(f.minimum > 1);
    let err = f
        .client
        .transfer(&f.collector, f.minimum - 1)?
        .send()
        .await
        .expect_err("underfunded collector created");
    assert!(
        matches!(err, gmsol_solana_utils::Error::Client(ref e) if matches!(e.get_transaction_error(), Some(TransactionError::InsufficientFundsForRent { .. })))
    );
    assert!(f.account(&f.collector).await?.is_none());
    f.assert_balances(0, f.minimum).await?;
    Ok(())
}
