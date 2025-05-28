/// Wormhole Ops.
pub mod wormhole;

/// Pyth Reciever Ops.
pub mod receiver;

/// Hermes.
pub mod hermes;

/// Utils.
pub mod utils;

mod pull_oracle_impl;

use std::{collections::HashMap, future::Future, ops::Deref};

use anchor_client::{
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signer::Signer,
    },
};
use either::Either;
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, SendBundleOptions},
    program::Program,
    transaction_builder::TransactionBuilder,
};
use gmsol_store::states::common::TokensWithFeed;
use hermes::BinaryPriceUpdate;
use pyth_sdk::Identifier;
use pythnet_sdk::wire::v1::AccumulatorUpdateData;

use self::wormhole::WORMHOLE_PROGRAM_ID;

pub use self::{
    pull_oracle_impl::{PriceUpdates, PythPullOracleWithHermes},
    receiver::PythReceiverOps,
    wormhole::WormholeOps,
};

use self::hermes::PriceUpdate;

const VAA_SPLIT_INDEX: usize = 755;

/// With Pyth Prices.
pub struct WithPythPrices<'a, C> {
    post: BundleBuilder<'a, C>,
    consume: BundleBuilder<'a, C>,
    close: BundleBuilder<'a, C>,
}

impl<S, C> WithPythPrices<'_, C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    /// Estimate execution fee.
    pub async fn estimated_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<u64> {
        let mut execution_fee = self
            .post
            .estimate_execution_fee(compute_unit_price_micro_lamports)
            .await?;
        execution_fee = execution_fee.saturating_add(
            self.consume
                .estimate_execution_fee(compute_unit_price_micro_lamports)
                .await?,
        );
        execution_fee = execution_fee.saturating_add(
            self.close
                .estimate_execution_fee(compute_unit_price_micro_lamports)
                .await?,
        );
        Ok(execution_fee)
    }

    /// Send all transactions.
    pub async fn send_all(
        self,
        compute_unit_price_micro_lamports: Option<u64>,
        skip_preflight: bool,
        enable_tracing: bool,
    ) -> Result<Vec<Signature>, (Vec<Signature>, crate::Error)> {
        let mut error: Option<crate::Error> = None;

        let mut signatures = match self
            .post
            .send_all_with_opts(SendBundleOptions {
                without_compute_budget: false,
                compute_unit_price_micro_lamports,
                config: RpcSendTransactionConfig {
                    skip_preflight,
                    ..Default::default()
                },
                disable_error_tracing: !enable_tracing,
                ..Default::default()
            })
            .await
        {
            Ok(signatures) => signatures,
            Err((signatures, err)) => {
                error = Some(err.into());
                signatures
            }
        };

        if error.is_none() {
            let mut consume_signatures = match self
                .consume
                .send_all_with_opts(SendBundleOptions {
                    without_compute_budget: false,
                    compute_unit_price_micro_lamports,
                    config: RpcSendTransactionConfig {
                        skip_preflight,
                        ..Default::default()
                    },
                    disable_error_tracing: !enable_tracing,
                    ..Default::default()
                })
                .await
            {
                Ok(signatures) => signatures,
                Err((signatures, err)) => {
                    error = Some(err.into());
                    signatures
                }
            };

            signatures.append(&mut consume_signatures);
        }

        let mut close_signatures = match self
            .close
            .send_all_with_opts(SendBundleOptions {
                without_compute_budget: false,
                compute_unit_price_micro_lamports,
                config: RpcSendTransactionConfig {
                    skip_preflight,
                    ..Default::default()
                },
                disable_error_tracing: !enable_tracing,
                ..Default::default()
            })
            .await
        {
            Ok(signatures) => signatures,
            Err((signatures, err)) => {
                match &error {
                    None => error = Some(err.into()),
                    Some(post_err) => {
                        error = Some(crate::Error::unknown(format!(
                            "post error: {post_err}, close error: {err}"
                        )));
                    }
                }
                signatures
            }
        };

        signatures.append(&mut close_signatures);

        match error {
            None => Ok(signatures
                .into_iter()
                .map(|with_slot| with_slot.into_value())
                .collect()),
            Some(err) => Err((
                signatures
                    .into_iter()
                    .map(|with_slot| with_slot.into_value())
                    .collect(),
                err,
            )),
        }
    }
}

/// Prices.
pub type Prices = HashMap<Identifier, Pubkey>;

/// Pyth Pull Oracle Context.
pub struct PythPullOracleContext {
    encoded_vaas: Vec<Keypair>,
    feeds: HashMap<Identifier, Keypair>,
    feed_ids: Vec<Identifier>,
}

impl PythPullOracleContext {
    /// Create a new [`PythPullOracleContext`].
    pub fn new(feed_ids: Vec<Identifier>) -> Self {
        let feeds = feed_ids.iter().map(|id| (*id, Keypair::new())).collect();
        Self {
            encoded_vaas: Vec::with_capacity(1),
            feeds,
            feed_ids,
        }
    }

    /// Create a new [`PythPullOracleContext`] from [`TokensWithFeed`].
    pub fn try_from_feeds(feeds: &TokensWithFeed) -> crate::Result<Self> {
        let feed_ids = utils::extract_pyth_feed_ids(feeds)?;
        Ok(Self::new(feed_ids))
    }

    /// Get feed ids.
    pub fn feed_ids(&self) -> &[Identifier] {
        &self.feed_ids
    }

    /// Create a new keypair for encoded vaa account.
    ///
    /// Return its index.
    pub fn add_encoded_vaa(&mut self) -> usize {
        self.encoded_vaas.push(Keypair::new());
        self.encoded_vaas.len() - 1
    }

    /// Get encoded vaas.
    pub fn encoded_vaas(&self) -> &[Keypair] {
        &self.encoded_vaas
    }
}

/// Pyth Pull Oracle Ops.
pub trait PythPullOracleOps<C> {
    /// Get Pyth Program.
    fn pyth(&self) -> &Program<C>;

    /// Get Wormhole Program.
    fn wormhole(&self) -> &Program<C>;

    /// Create transactions to post price updates and consume the prices.
    fn with_pyth_prices<'a, S, It, Fut>(
        &'a self,
        ctx: &'a PythPullOracleContext,
        update: &'a PriceUpdate,
        consume: impl FnOnce(Prices) -> Fut,
    ) -> impl Future<Output = crate::Result<WithPythPrices<'a, C>>>
    where
        C: Deref<Target = S> + Clone + 'a,
        S: Signer,
        It: IntoIterator<Item = TransactionBuilder<'a, C>>,
        Fut: Future<Output = crate::Result<It>>,
    {
        self.with_pyth_price_updates(ctx, [&update.binary], consume)
    }

    /// Create transactions to post price updates and consume the prices.
    fn with_pyth_price_updates<'a, S, It, Fut>(
        &'a self,
        ctx: &'a PythPullOracleContext,
        updates: impl IntoIterator<Item = &'a BinaryPriceUpdate>,
        consume: impl FnOnce(Prices) -> Fut,
    ) -> impl Future<Output = crate::Result<WithPythPrices<'a, C>>>
    where
        C: Deref<Target = S> + Clone + 'a,
        S: Signer,
        It: IntoIterator<Item = TransactionBuilder<'a, C>>,
        Fut: Future<Output = crate::Result<It>>,
    {
        use std::collections::hash_map::Entry;

        async {
            let wormhole = self.wormhole();
            let pyth = self.pyth();
            let mut prices = HashMap::with_capacity(ctx.feeds.len());
            let mut post = BundleBuilder::from_rpc_client(pyth.rpc());
            let mut consume_txns = BundleBuilder::from_rpc_client(pyth.rpc());
            let mut close = BundleBuilder::from_rpc_client(pyth.rpc());

            let datas = updates
                .into_iter()
                .flat_map(
                    |update| match utils::parse_accumulator_update_datas(update) {
                        Ok(datas) => Either::Left(datas.into_iter().map(Ok)),
                        Err(err) => Either::Right(std::iter::once(Err(err))),
                    },
                )
                .collect::<crate::Result<Vec<AccumulatorUpdateData>>>()?;

            // Merge by ids.
            let mut updates = HashMap::<_, _>::default();
            for data in datas.iter() {
                let proof = &data.proof;
                for update in utils::get_merkle_price_updates(proof) {
                    let feed_id = utils::parse_feed_id(update)?;
                    updates.insert(feed_id, (proof, update));
                }
            }

            // Write vaas.
            let mut encoded_vaas = HashMap::<_, _>::default();
            let mut vaas = HashMap::<_, _>::default();
            for (proof, _) in updates.values() {
                let vaa = utils::get_vaa_buffer(proof);
                if let Entry::Vacant(entry) = vaas.entry(vaa) {
                    let guardian_set_index = utils::get_guardian_set_index(proof)?;

                    let mut pubkey: Pubkey;
                    loop {
                        let keypair = Keypair::new();
                        pubkey = keypair.pubkey();
                        match encoded_vaas.entry(pubkey) {
                            Entry::Vacant(entry) => {
                                entry.insert(keypair);
                                break;
                            }
                            Entry::Occupied(_) => continue,
                        }
                    }

                    entry.insert((pubkey, guardian_set_index));
                }
            }

            for (vaa, (pubkey, guardian_set_index)) in vaas.iter() {
                let draft_vaa = encoded_vaas.remove(pubkey).expect("must exist");
                let create = wormhole
                    .create_encoded_vaa(draft_vaa, vaa.len() as u64)
                    .await?;
                let draft_vaa = pubkey;
                let write_1 = wormhole.write_encoded_vaa(draft_vaa, 0, &vaa[0..VAA_SPLIT_INDEX]);
                let write_2 = wormhole.write_encoded_vaa(
                    draft_vaa,
                    VAA_SPLIT_INDEX as u32,
                    &vaa[VAA_SPLIT_INDEX..],
                );
                let verify = wormhole.verify_encoded_vaa_v1(draft_vaa, *guardian_set_index);
                post.try_push(create.clear_output())?
                    .try_push(write_1)?
                    .try_push(write_2)?
                    .try_push(verify)?;
                let close_encoded_vaa = wormhole.close_encoded_vaa(draft_vaa);
                close.try_push(close_encoded_vaa)?;
            }

            // Post price updates.
            for (feed_id, (proof, update)) in updates {
                let price_update = Keypair::new();
                let vaa = utils::get_vaa_buffer(proof);
                let Some((encoded_vaa, _)) = vaas.get(vaa) else {
                    continue;
                };
                let (post_price_update, price_update) = pyth
                    .post_price_update(price_update, update, encoded_vaa)?
                    .swap_output(());
                prices.insert(feed_id, price_update);
                post.try_push(post_price_update)?;
                close.try_push(pyth.reclaim_rent(&price_update))?;
            }

            let consume = (consume)(prices).await?;
            consume_txns.push_many(consume, false)?;
            Ok(WithPythPrices {
                post,
                consume: consume_txns,
                close,
            })
        }
    }

    /// Execute with pyth price updates.
    fn execute_with_pyth_price_updates<'a, 'exec, T, S>(
        &'exec self,
        updates: impl IntoIterator<Item = &'a BinaryPriceUpdate>,
        execute: &mut T,
        compute_unit_price_micro_lamports: Option<u64>,
        skip_preflight: bool,
        enable_tracing: bool,
    ) -> impl Future<Output = crate::Result<()>>
    where
        C: Deref<Target = S> + Clone + 'exec,
        S: Signer,
        T: ExecuteWithPythPrices<'exec, C>,
    {
        async move {
            let mut execution_fee_estiamted = !execute.should_estiamte_execution_fee();
            let updates = updates.into_iter().collect::<Vec<_>>();
            let ctx = execute.context().await?;
            let mut with_prices;
            loop {
                with_prices = self
                    .with_pyth_price_updates(&ctx, updates.clone(), |prices| async {
                        let rpcs = execute.build_rpc_with_price_updates(prices).await?;
                        Ok(rpcs)
                    })
                    .await?;
                if execution_fee_estiamted {
                    break;
                } else {
                    let execution_fee = with_prices
                        .estimated_execution_fee(compute_unit_price_micro_lamports)
                        .await?;
                    execute.set_execution_fee(execution_fee);
                    tracing::info!(%execution_fee, "execution fee estimated");
                    execution_fee_estiamted = true;
                }
            }
            execute
                .execute_with_options(
                    with_prices,
                    compute_unit_price_micro_lamports,
                    skip_preflight,
                    enable_tracing,
                )
                .await?;
            Ok(())
        }
    }
}

/// Pyth Pull Oracle.
pub struct PythPullOracle<C> {
    wormhole: Program<C>,
    pyth: Program<C>,
}

impl<S, C> PythPullOracle<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    /// Create a new [`PythPullOracle`] client from [`Client`](crate::Client).
    pub fn try_new(client: &crate::Client<C>) -> crate::Result<Self> {
        Ok(Self {
            wormhole: client.program(WORMHOLE_PROGRAM_ID),
            pyth: client.program(pyth_solana_receiver_sdk::ID),
        })
    }
}

impl<S, C> PythPullOracleOps<C> for PythPullOracle<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn pyth(&self) -> &Program<C> {
        &self.pyth
    }

    fn wormhole(&self) -> &Program<C> {
        &self.wormhole
    }
}

/// Execute with pyth prices.
pub trait ExecuteWithPythPrices<'a, C> {
    /// Whether to estimate the execution fee.
    fn should_estiamte_execution_fee(&self) -> bool {
        true
    }

    /// Set execution fee.
    fn set_execution_fee(&mut self, lamports: u64);

    /// Get the oracle context.
    fn context(&mut self) -> impl Future<Output = crate::Result<PythPullOracleContext>>;

    /// Build RPC requests with price updates.
    fn build_rpc_with_price_updates(
        &mut self,
        price_updates: Prices,
    ) -> impl Future<Output = crate::Result<Vec<TransactionBuilder<'a, C, ()>>>>;

    /// Execute with options
    fn execute_with_options<S>(
        &mut self,
        txns: WithPythPrices<C>,
        compute_unit_price_micro_lamports: Option<u64>,
        skip_preflight: bool,
        enable_tracing: bool,
    ) -> impl Future<Output = crate::Result<()>>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        async move {
            match txns
                .send_all(
                    compute_unit_price_micro_lamports,
                    skip_preflight,
                    enable_tracing,
                )
                .await
            {
                Ok(signatures) => {
                    if enable_tracing {
                        tracing::info!("executed with txns {signatures:#?}");
                    }
                    Ok(())
                }
                Err((signatures, err)) => {
                    if enable_tracing {
                        tracing::error!(%err, "failed to execute, successful txns: {signatures:#?}");
                    }
                    Err(err)
                }
            }
        }
    }

    /// Execute.
    fn execute<S>(
        &mut self,
        txns: WithPythPrices<C>,
        skip_preflight: bool,
    ) -> impl Future<Output = crate::Result<()>>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        self.execute_with_options(txns, None, skip_preflight, true)
    }
}
