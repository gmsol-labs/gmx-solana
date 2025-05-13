use std::ops::Deref;

use crate::{
    bundle_builder::{BundleBuilder, BundleOptions},
    transaction_builder::TransactionBuilder,
};
use solana_sdk::signer::Signer;

use super::MakeBundleBuilder;

/// Surround transaction.
pub struct Surround<'a, C, T> {
    builder: T,
    pre_transaction_stack: Vec<TransactionBuilder<'a, C>>,
    post_transaction_queue: Vec<TransactionBuilder<'a, C>>,
}

impl<C, T> From<T> for Surround<'_, C, T> {
    fn from(builder: T) -> Self {
        Self {
            builder,
            pre_transaction_stack: vec![],
            post_transaction_queue: vec![],
        }
    }
}

impl<'a, C, T> Surround<'a, C, T> {
    /// Prepend a transaction to the pre-transaction list.
    pub fn pre_transaction(&mut self, transaction: TransactionBuilder<'a, C>) -> &mut Self {
        self.pre_transaction_stack.push(transaction);
        self
    }

    /// Append a transaction to the post-transaction list.
    pub fn post_transaction(&mut self, transaction: TransactionBuilder<'a, C>) -> &mut Self {
        self.post_transaction_queue.push(transaction);
        self
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone, T> MakeBundleBuilder<'a, C> for Surround<'a, C, T>
where
    T: MakeBundleBuilder<'a, C>,
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> crate::Result<BundleBuilder<'a, C>> {
        let mut bundle = self.builder.build_with_options(options).await?;

        if !self.pre_transaction_stack.is_empty() {
            let mut pre_bundle = bundle.try_clone_empty()?;
            // FILO insertion.
            for txn in self.pre_transaction_stack.iter().rev() {
                pre_bundle.push(txn.clone())?;
            }

            pre_bundle.append(bundle, false)?;
            bundle = pre_bundle;
        }

        bundle.push_many(self.post_transaction_queue.iter().cloned(), false)?;

        Ok(bundle)
    }
}
