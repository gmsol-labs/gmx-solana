use std::ops::Deref;

use crate::bundle_builder::{BundleBuilder, BundleOptions};
use solana_sdk::signer::Signer;

use super::MakeBundleBuilder;

/// Estimate Execution Fee.
pub struct EstimateFee<T> {
    builder: T,
    compute_unit_price_micro_lamports: Option<u64>,
    compute_unit_min_priority_lamports: Option<u64>,
}

impl<T> EstimateFee<T> {
    /// Estiamte fee before building the transaction.
    pub fn new(
        builder: T,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> Self {
        Self {
            builder,
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
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

// Lets a caller wrap `&mut builder` in `EstimateFee` and still read `builder` after sending.
impl<T: SetExecutionFee> SetExecutionFee for &mut T {
    fn is_execution_fee_estimation_required(&self) -> bool {
        (**self).is_execution_fee_estimation_required()
    }

    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        (**self).set_execution_fee(lamports);
        self
    }
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
            let lamports = tx.build()?.estimate_execution_fee(
                self.compute_unit_price_micro_lamports,
                self.compute_unit_min_priority_lamports,
            );
            self.builder.set_execution_fee(lamports);
            tracing::info!(%lamports, "execution fee estimated");
            tx = self.builder.build_with_options(options).await?;
        }

        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fee {
        lamports: u64,
        required: bool,
    }

    impl SetExecutionFee for Fee {
        fn is_execution_fee_estimation_required(&self) -> bool {
            self.required
        }

        fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
            self.lamports = lamports;
            self
        }
    }

    /// Same bound `EstimateFee` uses, so `&mut Fee` must go through the blanket impl.
    fn estimate<T: SetExecutionFee>(mut builder: T) -> bool {
        builder.set_execution_fee(5_000);
        builder.is_execution_fee_estimation_required()
    }

    #[test]
    fn mut_ref_forwards_to_the_builder() {
        let mut fee = Fee {
            lamports: 0,
            required: false,
        };
        assert!(!estimate(&mut fee));
        assert_eq!(fee.lamports, 5_000);
    }
}
