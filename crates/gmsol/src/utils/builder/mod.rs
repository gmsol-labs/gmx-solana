/// Estimate Execution Fee.
pub mod estimate_fee;

/// Set Oracle Price Feed.
pub mod oracle;

use std::future::Future;

pub use estimate_fee::{EstimateFee, SetExecutionFee};
use gmsol_solana_utils::bundle_builder::BundleBuilder;
pub use oracle::{
    FeedAddressMap, FeedIds, PostPullOraclePrices, PriceUpdateInstructions, PullOracle,
    PullOraclePriceConsumer, WithPullOracle,
};

/// Builder for [`BundleBuilder`]s.
pub trait MakeBundleBuilder<'a, C> {
    /// Build.
    fn build(&mut self) -> impl Future<Output = crate::Result<BundleBuilder<'a, C>>>;
}
