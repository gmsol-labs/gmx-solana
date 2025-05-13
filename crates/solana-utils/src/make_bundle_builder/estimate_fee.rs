use std::ops::Deref;

use crate::bundle_builder::{BundleBuilder, BundleOptions};
use anchor_client::solana_sdk::signer::Signer;

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
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<'a, C>> {
        let mut tx = self.builder.build_with_options(options.clone()).await?;

        if self.builder.is_execution_fee_estimation_required() {
            let lamports = tx
                .estimate_execution_fee(self.compute_unit_price_micro_lamports)
                .await?;
            self.builder.set_execution_fee(lamports);
            tracing::info!(%lamports, "execution fee estimated");
            tx = self.builder.build_with_options(options).await?;
        }

        Ok(tx)
    }
}
