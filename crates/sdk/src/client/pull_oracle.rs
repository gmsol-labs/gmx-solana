use std::{collections::HashMap, future::Future, ops::Deref};

use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};

use gmsol_utils::{oracle::PriceProviderKind, token_config::TokensWithFeed};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use time::OffsetDateTime;

use super::{feeds_parser::FeedAddressMap, Client};

/// Build with pull oracle instructions.
pub struct WithPullOracle<O, T>
where
    O: PullOracle,
{
    builder: T,
    pull_oracle: O,
    price_updates: O::PriceUpdates,
    temporary_accounts: Vec<Pubkey>,
}

impl<O: PullOracle, T> WithPullOracle<O, T> {
    /// Construct transactions with the given pull oracle and price updates.
    pub fn with_price_updates(pull_oracle: O, builder: T, price_updates: O::PriceUpdates) -> Self {
        Self {
            builder,
            pull_oracle,
            price_updates,
            temporary_accounts: vec![],
        }
    }

    /// Temporary oracle accounts created by the most recent build.
    ///
    /// The built bundle closes them in its last batch. If sending stops before that batch,
    /// close them with
    /// [`PythPullOracle::close_temporary_accounts`](crate::client::pyth::PythPullOracle::close_temporary_accounts).
    /// Empty before the first build.
    pub fn temporary_accounts(&self) -> &[Pubkey] {
        &self.temporary_accounts
    }

    /// Fetch the required price updates and use them to construct transactions.
    pub async fn from_consumer(
        pull_oracle: O,
        mut builder: T,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<(Self, FeedIds)>
    where
        T: PullOraclePriceConsumer,
    {
        let feed_ids = builder.feed_ids().await?;
        let price_updates = pull_oracle.fetch_price_updates(&feed_ids, after).await?;

        Ok((
            Self::with_price_updates(pull_oracle, builder, price_updates),
            feed_ids,
        ))
    }

    /// Fetch the required price updates and use them to construct transactions.
    pub async fn new(
        pull_oracle: O,
        builder: T,
        after: Option<OffsetDateTime>,
    ) -> crate::Result<Self>
    where
        T: PullOraclePriceConsumer,
    {
        Ok(Self::from_consumer(pull_oracle, builder, after).await?.0)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone, O, T> MakeBundleBuilder<'a, C>
    for WithPullOracle<O, T>
where
    O: PostPullOraclePrices<'a, C>,
    T: PullOraclePriceConsumer + MakeBundleBuilder<'a, C>,
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let (instructions, map) = self
            .pull_oracle
            .fetch_price_update_instructions(&self.price_updates, options.clone())
            .await
            .map_err(gmsol_solana_utils::Error::custom)?;

        for (kind, map) in map {
            self.builder
                .process_feeds(kind, map)
                .map_err(gmsol_solana_utils::Error::custom)?;
        }

        let consume = self.builder.build_with_options(options).await?;

        let PriceUpdateInstructions {
            post: mut tx,
            close,
            temporary_accounts,
        } = instructions;
        // Overwrite, not extend: `EstimateFee` builds twice and only the last build is sent.
        self.temporary_accounts = temporary_accounts;

        tx.append(consume, false)?;
        tx.append(close, true)?;

        Ok(tx)
    }
}

impl<O: PullOracle, T: SetExecutionFee> SetExecutionFee for WithPullOracle<O, T> {
    fn is_execution_fee_estimation_required(&self) -> bool {
        self.builder.is_execution_fee_estimation_required()
    }

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
    temporary_accounts: Vec<Pubkey>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> PriceUpdateInstructions<'a, C> {
    /// Create a new empty price update instructions.
    pub fn new(client: &'a Client<C>, options: BundleOptions) -> Self {
        Self {
            post: client.bundle_with_options(options.clone()),
            close: client.bundle_with_options(options),
            temporary_accounts: vec![],
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

    /// Get the mutable references to the [`BundleBuilder`]s.
    pub fn split_mut(&mut self) -> (&mut BundleBuilder<'a, C>, &mut BundleBuilder<'a, C>) {
        (&mut self.post, &mut self.close)
    }

    /// Returns the inner [`BundleBuilder`].
    pub fn split(self) -> (BundleBuilder<'a, C>, BundleBuilder<'a, C>) {
        (self.post, self.close)
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

    /// Record an account that `post` creates and `close` closes, so it can still be closed
    /// when the bundle stops before its `close` batch.
    pub fn push_temporary_account(&mut self, account: Pubkey) {
        self.temporary_accounts.push(account);
    }

    /// Accounts recorded with [`Self::push_temporary_account`].
    pub fn temporary_accounts(&self) -> &[Pubkey] {
        &self.temporary_accounts
    }

    /// Merge.
    pub fn merge(&mut self, other: Self) -> gmsol_solana_utils::Result<()> {
        self.post.append(other.post, false)?;
        self.close.append(other.close, false)?;
        self.temporary_accounts.extend(other.temporary_accounts);
        Ok(())
    }
}

/// Post pull oracle price updates.
pub trait PostPullOraclePrices<'a, C>: PullOracle {
    /// Fetch instructions to post the price updates.
    fn fetch_price_update_instructions(
        &self,
        price_updates: &Self::PriceUpdates,
        options: BundleOptions,
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

impl<T: PullOraclePriceConsumer> PullOraclePriceConsumer for &mut T {
    fn feed_ids(&mut self) -> impl Future<Output = crate::Result<FeedIds>> {
        (**self).feed_ids()
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        (**self).process_feeds(provider, map)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gmsol_solana_utils::cluster::Cluster;
    use solana_sdk::signature::Keypair;

    use super::*;

    type TestClient = Client<Arc<Keypair>>;

    /// Records one new account per build, like Pyth does with fresh keypairs.
    struct FakeOracle<'a>(&'a TestClient);

    impl PullOracle for FakeOracle<'_> {
        type PriceUpdates = ();

        async fn fetch_price_updates(
            &self,
            _feed_ids: &FeedIds,
            _after: Option<OffsetDateTime>,
        ) -> crate::Result<()> {
            Ok(())
        }
    }

    impl<'a> PostPullOraclePrices<'a, Arc<Keypair>> for FakeOracle<'a> {
        async fn fetch_price_update_instructions(
            &self,
            _price_updates: &(),
            options: BundleOptions,
        ) -> crate::Result<(
            PriceUpdateInstructions<'a, Arc<Keypair>>,
            HashMap<PriceProviderKind, FeedAddressMap>,
        )> {
            let mut ixns = PriceUpdateInstructions::new(self.0, options);
            ixns.push_temporary_account(Pubkey::new_unique());
            Ok((ixns, HashMap::new()))
        }
    }

    struct FakeConsumer<'a>(&'a TestClient);

    impl PullOraclePriceConsumer for FakeConsumer<'_> {
        async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
            unreachable!("built with `with_price_updates`")
        }

        fn process_feeds(&mut self, _: PriceProviderKind, _: FeedAddressMap) -> crate::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeBundleBuilder<'a, Arc<Keypair>> for FakeConsumer<'a> {
        async fn build_with_options(
            &mut self,
            options: BundleOptions,
        ) -> gmsol_solana_utils::Result<BundleBuilder<'a, Arc<Keypair>>> {
            Ok(self.0.bundle_with_options(options))
        }
    }

    #[tokio::test]
    async fn keeps_only_the_last_build_temporary_accounts() {
        let client = Client::new(Cluster::Devnet, Arc::new(Keypair::new())).unwrap();
        let mut with_oracle =
            WithPullOracle::with_price_updates(FakeOracle(&client), FakeConsumer(&client), ());
        assert!(with_oracle.temporary_accounts().is_empty());

        with_oracle.build().await.unwrap();
        let first = with_oracle.temporary_accounts().to_vec();
        with_oracle.build().await.unwrap();
        let second = with_oracle.temporary_accounts().to_vec();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first, second);
    }

    #[test]
    fn merge_keeps_both_sides_temporary_accounts() {
        let client = Client::new(Cluster::Devnet, Arc::new(Keypair::new())).unwrap();
        let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut left = PriceUpdateInstructions::new(&client, Default::default());
        left.push_temporary_account(a);
        let mut right = PriceUpdateInstructions::new(&client, Default::default());
        right.push_temporary_account(b);

        left.merge(right).unwrap();

        assert_eq!(left.temporary_accounts(), [a, b]);
    }
}
