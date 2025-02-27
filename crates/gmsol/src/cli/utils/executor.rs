use std::{ops::Deref, sync::Arc};

use gmsol::{
    chainlink::{self, pull_oracle::ChainlinkPullOracleFactory},
    pyth::{pull_oracle::PythPullOracleWithHermes, Hermes, PythPullOracle},
    switchboard::pull_oracle::SwitchcboardPullOracleFactory,
    utils::{
        builder::{
            EstimateFee, MakeBundleBuilder, PullOraclePriceConsumer, SetExecutionFee,
            WithPullOracle,
        },
        instruction::InstructionSerialization,
    },
};
use gmsol_solana_utils::bundle_builder::BundleOptions;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use crate::InstructionBufferCtx;

pub(crate) struct Executor<'a, C> {
    store: Pubkey,
    client: &'a gmsol::Client<C>,
    chainlink: Option<(chainlink::Client, Arc<ChainlinkPullOracleFactory>)>,
    pyth: PythPullOracle<C>,
    hermes: Hermes,
    switchboard: Option<SwitchcboardPullOracleFactory>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> Executor<'a, C> {
    pub(crate) async fn new_with_envs(
        store: &Pubkey,
        client: &'a gmsol::Client<C>,
        testnet: bool,
        feed_index: u16,
        use_switchboard: bool,
    ) -> gmsol::Result<Self> {
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
            store: *store,
            client,
            chainlink,
            pyth,
            hermes: Default::default(),
            switchboard,
        })
    }

    pub(crate) async fn execute<'b>(
        &'b self,
        consumer: impl PullOraclePriceConsumer + MakeBundleBuilder<'b, C> + SetExecutionFee,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        max_transaction_size: Option<usize>,
        compute_unit_price: Option<u64>,
    ) -> gmsol::Result<()> {
        let options = BundleOptions {
            max_packet_size: max_transaction_size,
            ..Default::default()
        };

        let pyth = PythPullOracleWithHermes::from_parts(self.client, &self.hermes, &self.pyth);

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

        super::send_or_serialize_bundle_with_default_callback(
            &self.store,
            bundle,
            ctx,
            serialize_only,
            skip_preflight,
        )
        .await?;

        Ok(())
    }
}
