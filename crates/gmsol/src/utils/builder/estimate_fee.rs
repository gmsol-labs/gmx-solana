use std::ops::Deref;

use anchor_client::solana_sdk::signer::Signer;
use gmsol_solana_utils::bundle_builder::BundleBuilder;

use super::MakeBundleBuilder;

/// Estimate Execution Fee.
pub struct EstimateFee<T> {
    builder: T,
    compute_unit_price_micro_lamports: Option<u64>,
}

impl<T> EstimateFee<T> {
    /// Estiamte fee before building the transaction.
    pub fn new(builder: T, compute_unit_price_micro_lamports: Option<u64>) -> Self {
        Self {
            builder,
            compute_unit_price_micro_lamports,
        }
    }
}

/// Set Execution Fee.
pub trait SetExecutionFee {
    /// Whether the execution fee needed to be estiamted.
    fn is_execution_fee_estimation_required(&self) -> bool {
        true
    }

    /// Set execution fee.
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self;
}

impl<'a, C: Deref<Target = impl Signer> + Clone, T> MakeBundleBuilder<'a, C> for EstimateFee<T>
where
    T: SetExecutionFee,
    T: MakeBundleBuilder<'a, C>,
{
    async fn build(&mut self) -> crate::Result<BundleBuilder<'a, C>> {
        let mut tx = self.builder.build().await?;

        if self.builder.is_execution_fee_estimation_required() {
            let lamports = tx
                .estimate_execution_fee(self.compute_unit_price_micro_lamports)
                .await?;
            self.builder.set_execution_fee(lamports);
            tracing::info!(%lamports, "execution fee estimated");
            tx = self.builder.build().await?;
        }

        Ok(tx)
    }
}
