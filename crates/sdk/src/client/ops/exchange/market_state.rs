use std::ops::Deref;

use gmsol_solana_utils::{
    client_traits::FromRpcClientWith, make_bundle_builder::MakeBundleBuilder,
    transaction_builder::TransactionBuilder, IntoAtomicGroup,
};
use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::signer::Signer;

use crate::{
    builders::market_state::{UpdateClosedState, UpdateClosedStateHint},
    client::pull_oracle::{FeedIds, PullOraclePriceConsumer},
    utils::token_map::FeedAddressMap,
};

/// Builder for `update_closed_state` instruction.
pub struct UpdateClosedStateBuilder<'a, C> {
    client: &'a crate::Client<C>,
    builder: UpdateClosedState,
    hint: Option<UpdateClosedStateHint>,
}

impl<'a, C> UpdateClosedStateBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, builder: UpdateClosedState) -> Self {
        Self {
            client,
            builder,
            hint: None,
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> UpdateClosedStateBuilder<'a, C> {
    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<UpdateClosedStateHint> {
        if let Some(hint) = self.hint.as_ref() {
            return Ok(hint.clone());
        }
        let hint =
            UpdateClosedStateHint::from_rpc_client_with(&self.builder, self.client.rpc()).await?;
        self.hint = Some(hint.clone());
        Ok(hint)
    }

    async fn build_txn(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let ag = self.builder.clone().into_atomic_group(&hint)?;
        let txn = self.client.store_transaction().pre_atomic_group(ag, true);
        Ok(txn)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for UpdateClosedStateBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: gmsol_solana_utils::bundle_builder::BundleOptions,
    ) -> gmsol_solana_utils::Result<gmsol_solana_utils::bundle_builder::BundleBuilder<'a, C>> {
        let mut tx = self.client.bundle_with_options(options);

        tx.try_push(
            self.build_txn()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
        )?;

        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for UpdateClosedStateBuilder<'_, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(
            self.builder.store_program.store.0,
            hint.to_tokens_with_feeds()?,
        ))
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.builder.insert_feed_parser(provider, map)?;
        Ok(())
    }
}
