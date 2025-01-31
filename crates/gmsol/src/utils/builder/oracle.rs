use std::{collections::HashMap, future::Future, ops::Deref};

use anchor_client::solana_sdk::{pubkey::Pubkey, signer::Signer};
use gmsol_solana_utils::{bundle_builder::BundleBuilder, transaction_builder::TransactionBuilder};
use gmsol_store::states::{common::TokensWithFeed, PriceProviderKind};
use time::OffsetDateTime;

use super::{MakeBundleBuilder, SetExecutionFee};

/// A mapping from feed id to the corresponding feed address.
pub type FeedAddressMap = std::collections::HashMap<Pubkey, Pubkey>;

/// Build with pull oracle instructions.
pub struct WithPullOracle<O, T>
where
    O: PullOracle,
{
    builder: T,
    pull_oracle: O,
    price_updates: O::PriceUpdates,
}

impl<O: PullOracle, T> WithPullOracle<O, T> {
    /// Construct transactions with the given pull oracle and price updates.
    pub fn with_price_updates(pull_oracle: O, builder: T, price_updates: O::PriceUpdates) -> Self {
        Self {
            builder,
            pull_oracle,
            price_updates,
        }
    }

    /// Fetch the required price updates and use them to construct transactions.
    pub async fn new(
        pull_oracle: O,
        mut builder: T,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self>
    where
        T: PullOraclePriceConsumer,
    {
        let feed_ids = builder.feed_ids().await?;
        let price_updates = pull_oracle.fetch_price_updates(&feed_ids, after).await?;

        Ok(Self::with_price_updates(
            pull_oracle,
            builder,
            price_updates,
        ))
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone, O, T> MakeBundleBuilder<'a, C>
    for WithPullOracle<O, T>
where
    O: PostPullOraclePrices<'a, C>,
    T: PullOraclePriceConsumer + MakeBundleBuilder<'a, C>,
{
    async fn build(&mut self) -> crate::Result<BundleBuilder<'a, C>> {
        let (instructions, map) = self
            .pull_oracle
            .fetch_price_update_instructions(&self.price_updates)
            .await?;

        for (kind, map) in map {
            self.builder.process_feeds(kind, map)?;
        }

        let consume = self.builder.build().await?;

        let PriceUpdateInstructions {
            post: mut tx,
            close,
        } = instructions;

        tx.append(consume, false)?;
        tx.append(close, true)?;

        Ok(tx)
    }
}

impl<O: PullOracle, T: SetExecutionFee> SetExecutionFee for WithPullOracle<O, T> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.builder.set_execution_fee(lamports);
        self
    }
}

impl<O: PullOracle, T: PullOraclePriceConsumer> PullOraclePriceConsumer for WithPullOracle<O, T> {
    fn feed_ids(&mut self) -> impl Future<Output = crate::Result<FeedIds>> {
        self.builder.feed_ids()
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.builder.process_feeds(provider, map)
    }
}

/// Feed IDs.
#[derive(Debug, Clone)]
pub struct FeedIds {
    store: Pubkey,
    tokens_with_feed: TokensWithFeed,
}

impl FeedIds {
    /// Get the store address.
    pub fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Create from `store` and `tokens_with_feed`.
    pub fn new(store: Pubkey, tokens_with_feed: TokensWithFeed) -> Self {
        Self {
            store,
            tokens_with_feed,
        }
    }
}

impl Deref for FeedIds {
    type Target = TokensWithFeed;

    fn deref(&self) -> &Self::Target {
        &self.tokens_with_feed
    }
}

/// Pull Oracle.
pub trait PullOracle {
    /// Price Updates.
    type PriceUpdates;

    /// Fetch Price Update.
    fn fetch_price_updates(
        &self,
        feed_ids: &FeedIds,
        after: Option<OffsetDateTime>,
    ) -> impl Future<Output = crate::Result<Self::PriceUpdates>>;
}

/// Price Update Instructions.
pub struct PriceUpdateInstructions<'a, C> {
    post: BundleBuilder<'a, C>,
    close: BundleBuilder<'a, C>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PriceUpdateInstructions<'a, C> {
    /// Create a new empty price update instructions.
    pub fn new(client: &'a crate::Client<C>) -> Self {
        Self {
            post: client.bundle(),
            close: client.bundle(),
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PriceUpdateInstructions<'a, C> {
    /// Push a post price update instruction.
    #[allow(clippy::result_large_err)]
    pub fn try_push_post(
        &mut self,
        instruction: TransactionBuilder<'a, C>,
    ) -> Result<(), (TransactionBuilder<'a, C>, gmsol_solana_utils::Error)> {
        self.post.try_push(instruction)?;
        Ok(())
    }

    /// Push a close instruction.
    #[allow(clippy::result_large_err)]
    pub fn try_push_close(
        &mut self,
        instruction: TransactionBuilder<'a, C>,
    ) -> Result<(), (TransactionBuilder<'a, C>, gmsol_solana_utils::Error)> {
        self.close.try_push(instruction)?;
        Ok(())
    }

    /// Merge.
    pub fn merge(&mut self, other: Self) -> gmsol_solana_utils::Result<()> {
        self.post.append(other.post, false)?;
        self.close.append(other.close, false)?;
        Ok(())
    }
}

/// Post pull oracle price updates.
pub trait PostPullOraclePrices<'a, C>: PullOracle {
    /// Fetch instructions to post the price updates.
    fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
    ) -> impl Future<
        Output = crate::Result<(
            PriceUpdateInstructions<'a, C>,
            HashMap<PriceProviderKind, FeedAddressMap>,
        )>,
    >;
}

/// Pull Oracle Price Consumer.
pub trait PullOraclePriceConsumer {
    /// Returns a reference to tokens and their associated feed IDs that require price updates.
    fn feed_ids(&mut self) -> impl Future<Output = crate::Result<FeedIds>>;

    /// Processes the feed address map returned from the pull oracle.
    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()>;
}
