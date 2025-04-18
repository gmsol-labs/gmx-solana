/// Estimate Execution Fee.
pub mod estimate_fee;

/// Set Oracle Price Feed.
pub mod oracle;

/// Surround transaction.
pub mod surround;

use std::{future::Future, ops::Deref};

pub use self::{
    estimate_fee::{EstimateFee, SetExecutionFee},
    oracle::{
        FeedAddressMap, FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
        PullOraclePriceConsumer, WithPullOracle,
    },
    surround::Surround,
};

use gmsol_solana_utils::transaction_builder::TransactionBuilder;
pub use gmsol_solana_utils::{bundle_builder::BundleBuilder, bundle_builder::BundleOptions};
use solana_sdk::signer::Signer;

/// Builder for [`BundleBuilder`]s.
pub trait MakeBundleBuilder<'a, C> {
    /// Build with options.
    fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> impl Future<Output = crate::Result<BundleBuilder<'a, C>>>;

    /// Build.
    fn build(&mut self) -> impl Future<Output = crate::Result<BundleBuilder<'a, C>>> {
        self.build_with_options(Default::default())
    }
}

impl<'a, C, T> MakeBundleBuilder<'a, C> for &mut T
where
    T: MakeBundleBuilder<'a, C>,
{
    fn build(&mut self) -> impl Future<Output = crate::Result<BundleBuilder<'a, C>>> {
        (**self).build()
    }

    fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> impl Future<Output = crate::Result<BundleBuilder<'a, C>>> {
        (**self).build_with_options(options)
    }
}

/// Extension trait for [`MakeBundleBuilder`].
pub trait MakeBundleBuilderExt<'a, C>: MakeBundleBuilder<'a, C> {
    /// Surround the current builder.
    fn surround(self) -> Surround<'a, C, Self>
    where
        Self: Sized,
    {
        self.into()
    }
}

impl<'a, C, T: MakeBundleBuilder<'a, C> + ?Sized> MakeBundleBuilderExt<'a, C> for T {}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for TransactionBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<'a, C>> {
        Ok(self.clone().into_bundle_with_options(options)?)
    }
}

/// Make bundle builder that can only be used once.
pub struct OnceMakeBundleBuilder<'a, C>(Option<BundleBuilder<'a, C>>);

impl<'a, C> From<BundleBuilder<'a, C>> for OnceMakeBundleBuilder<'a, C> {
    fn from(value: BundleBuilder<'a, C>) -> Self {
        Self(Some(value))
    }
}

/// Create a [`MakeBundleBuilder`] from a [`BundleBuilder`].
pub fn once_make_bundle<C>(bundle: BundleBuilder<C>) -> OnceMakeBundleBuilder<'_, C> {
    bundle.into()
}

impl<'a, C> MakeBundleBuilder<'a, C> for OnceMakeBundleBuilder<'a, C> {
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<'a, C>> {
        let mut bundle = self.0.take().ok_or_else(|| {
            crate::Error::unknown("`OnceMakeBundleBuilder` can only be used once")
        })?;
        bundle.set_options(options);
        Ok(bundle)
    }
}
