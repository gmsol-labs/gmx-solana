use std::sync::Arc;

use gmsol_sdk::{
    client::{
        chainlink::{self, pull_oracle::ChainlinkPullOracleFactory},
        pull_oracle::{PullOraclePriceConsumer, WithPullOracle},
        pyth::{pull_oracle::PythPullOracleWithHermes, Hermes, PythPullOracle},
        switchboard::pull_oracle::SwitchcboardPullOracleFactory,
    },
    solana_utils::{
        bundle_builder::BundleOptions,
        make_bundle_builder::{EstimateFee, MakeBundleBuilder, SetExecutionFee},
        signer::LocalSignerRef,
    },
};

use crate::CommandClient;

/// Arguments for executor.
#[derive(clap::Args, Clone, Debug)]
pub struct ExecutorArgs {
    #[cfg_attr(feature = "devnet", arg(long, default_value_t = true))]
    #[cfg_attr(not(feature = "devnet"), arg(long, default_value_t = false))]
    oracle_testnet: bool,
    /// Whether to disable Switchboard support.
    #[arg(long)]
    disable_switchboard: bool,
    /// Feed index.
    #[arg(long, default_value_t = 0)]
    feed_index: u16,
}

impl ExecutorArgs {
    pub(crate) async fn build<'a>(
        &self,
        client: &'a CommandClient,
    ) -> gmsol_sdk::Result<Executor<'a>> {
        Executor::new_with_envs(
            client,
            self.oracle_testnet,
            self.feed_index,
            !self.disable_switchboard,
        )
        .await
    }
}

pub(crate) struct Executor<'a> {
    client: &'a CommandClient,
    chainlink: Option<(chainlink::Client, Arc<ChainlinkPullOracleFactory>)>,
    pyth: PythPullOracle<LocalSignerRef>,
    hermes: Hermes,
    switchboard: Option<SwitchcboardPullOracleFactory>,
}

impl<'a> Executor<'a> {
    pub(crate) async fn new_with_envs(
        client: &'a CommandClient,
        testnet: bool,
        feed_index: u16,
        use_switchboard: bool,
    ) -> gmsol_sdk::Result<Self> {
        let store = &client.store;
        let pyth = PythPullOracle::try_new(client)?;
        let chainlink = if testnet {
            chainlink::Client::from_testnet_defaults()
        } else {
            chainlink::Client::from_defaults()
        }
        .inspect_err(|_err| {
            tracing::warn!("Chainlink envs is not provided, continue without chainlink support")
        })
        .ok()
        .map(|client| {
            (
                client,
                ChainlinkPullOracleFactory::new(store, feed_index).arced(),
            )
        });

        let switchboard = if use_switchboard {
            match SwitchcboardPullOracleFactory::from_env() {
                Ok(switchboard) => Some(switchboard),
                Err(_) => Some(
                    SwitchcboardPullOracleFactory::from_default_queue(
                        &client.store_program().rpc(),
                    )
                    .await?,
                ),
            }
        } else {
            None
        };

        Ok(Self {
            client,
            chainlink,
            pyth,
            hermes: Default::default(),
            switchboard,
        })
    }

    pub(crate) async fn execute<'b>(
        &'b self,
        consumer: impl PullOraclePriceConsumer + MakeBundleBuilder<'b, LocalSignerRef> + SetExecutionFee,
        options: BundleOptions,
    ) -> gmsol_sdk::Result<()> {
        let pyth = PythPullOracleWithHermes::from_parts(self.client, &self.hermes, &self.pyth);
        let compute_unit_price = self
            .client
            .send_bundle_options()
            .compute_unit_price_micro_lamports;
        let chainlink = self
            .chainlink
            .as_ref()
            .map(|(client, factory)| factory.clone().make_oracle(client, self.client, true));

        let switchboard = self
            .switchboard
            .as_ref()
            .map(|factory| factory.make_oracle(self.client));

        let with_pyth = WithPullOracle::new(pyth, consumer, None).await?;

        let bundle = match (chainlink.as_ref(), switchboard) {
            (None, None) => {
                let mut estiamted_fee = EstimateFee::new(with_pyth, compute_unit_price);
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), None) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), with_pyth, None).await?;

                let mut bundle = chainlink
                    .prepare_feeds_bundle(&feed_ids, options.clone())
                    .await?;

                let mut estiamted_fee = EstimateFee::new(with_chainlink, compute_unit_price);

                bundle.append(estiamted_fee.build_with_options(options).await?, false)?;

                bundle
            }
            (None, Some(switchboard)) => {
                let with_switchboard = WithPullOracle::new(switchboard, with_pyth, None).await?;

                let mut estiamted_fee = EstimateFee::new(with_switchboard, compute_unit_price);
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), Some(switchboard)) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), with_pyth, None).await?;

                let with_switchboard =
                    WithPullOracle::new(switchboard, with_chainlink, None).await?;

                let mut estiamted_fee = EstimateFee::new(with_switchboard, compute_unit_price);

                let mut bundle = chainlink
                    .prepare_feeds_bundle(&feed_ids, options.clone())
                    .await?;

                bundle.append(estiamted_fee.build_with_options(options).await?, false)?;

                bundle
            }
        };

        self.client.send_or_serialize(bundle).await?;

        Ok(())
    }
}
