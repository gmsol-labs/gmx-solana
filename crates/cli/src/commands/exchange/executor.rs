use std::sync::Arc;

use gmsol_sdk::{
    client::{
        chainlink::{self, pull_oracle::ChainlinkPullOracleFactory},
        pull_oracle::{PullOraclePriceConsumer, WithPullOracle},
        pyth::{
            pull_oracle::{utils::extract_pyth_feed_ids, PythPullOracleWithHermes},
            Hermes, PythPullOracle,
        },
        switchboard::pull_oracle::SwitchcboardPullOracleFactory,
    },
    core::token_config::TokensWithFeed,
    solana_utils::{
        bundle_builder::BundleOptions,
        make_bundle_builder::{EstimateFee, MakeBundleBuilder, SetExecutionFee},
        signer::LocalSignerRef,
    },
};

use crate::{
    config::{Config, MISSING_PYTH_API_KEY},
    CommandClient,
};

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
    /// Whether to enable idempotent price updates for Chainlink.
    #[arg(long, default_value_t = false)]
    chainlink_idempotent: bool,
}

impl ExecutorArgs {
    pub(crate) async fn build<'a>(
        &self,
        client: &'a CommandClient,
        config: &Config,
    ) -> gmsol_sdk::Result<Executor<'a>> {
        let hermes = config.optional_hermes().map_err(gmsol_sdk::Error::custom)?;
        Executor::new_with_envs(
            client,
            hermes,
            self.oracle_testnet,
            self.feed_index,
            !self.disable_switchboard,
            self.chainlink_idempotent,
        )
        .await
    }
}

pub(crate) struct Executor<'a> {
    client: &'a CommandClient,
    chainlink: Option<(chainlink::Client, Arc<ChainlinkPullOracleFactory>)>,
    pyth: PythPullOracle<LocalSignerRef>,
    hermes: Hermes,
    /// When `false`, `hermes` is unauthenticated and must not be used:
    /// `execute` rejects consumers with Pyth feeds.
    hermes_has_api_key: bool,
    switchboard: Option<SwitchcboardPullOracleFactory>,
    chainlink_idempotent: bool,
}

impl<'a> Executor<'a> {
    pub(crate) async fn new_with_envs(
        client: &'a CommandClient,
        hermes: Option<Hermes>,
        testnet: bool,
        feed_index: u16,
        use_switchboard: bool,
        chainlink_idempotent: bool,
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
                ChainlinkPullOracleFactory::new(store, feed_index, testnet).arced(),
            )
        });

        let switchboard = if use_switchboard {
            match SwitchcboardPullOracleFactory::from_env() {
                Ok(switchboard) => Some(switchboard),
                Err(_) => Some(
                    SwitchcboardPullOracleFactory::from_default_queue(
                        &client.store_program().rpc(),
                        testnet,
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
            hermes_has_api_key: hermes.is_some(),
            hermes: hermes.unwrap_or_default(),
            switchboard,
            chainlink_idempotent,
        })
    }

    pub(crate) async fn execute<'b>(
        &'b self,
        mut consumer: impl PullOraclePriceConsumer
            + MakeBundleBuilder<'b, LocalSignerRef>
            + SetExecutionFee,
        options: BundleOptions,
    ) -> gmsol_sdk::Result<()> {
        if !self.hermes_has_api_key {
            // Consumers cache their hint, so this second `feed_ids` call is cheap.
            let feed_ids = consumer.feed_ids().await?;
            ensure_no_pyth_feeds(&feed_ids)?;
        }
        let pyth = PythPullOracleWithHermes::from_parts(self.client, &self.hermes, &self.pyth);
        let send_bundle_options = self.client.send_bundle_options();
        let compute_unit_price = send_bundle_options.compute_unit_price_micro_lamports;
        let compute_unit_min_priority_lamports =
            send_bundle_options.compute_unit_min_priority_lamports;
        let chainlink = self.chainlink.as_ref().map(|(client, factory)| {
            factory
                .clone()
                .make_oracle(client, self.client, true)
                .with_idempotent(self.chainlink_idempotent)
        });

        let switchboard = self
            .switchboard
            .as_ref()
            .map(|factory| factory.make_oracle(self.client));

        let with_pyth = WithPullOracle::new(pyth, consumer, None).await?;

        let bundle = match (chainlink.as_ref(), switchboard) {
            (None, None) => {
                let mut estiamted_fee = EstimateFee::new(
                    with_pyth,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), None) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), with_pyth, None).await?;

                let mut bundle = chainlink
                    .prepare_feeds_bundle(&feed_ids, options.clone())
                    .await?;

                let mut estiamted_fee = EstimateFee::new(
                    with_chainlink,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );

                bundle.append(estiamted_fee.build_with_options(options).await?, false)?;

                bundle
            }
            (None, Some(switchboard)) => {
                let with_switchboard = WithPullOracle::new(switchboard, with_pyth, None).await?;

                let mut estiamted_fee = EstimateFee::new(
                    with_switchboard,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), Some(switchboard)) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), with_pyth, None).await?;

                let with_switchboard =
                    WithPullOracle::new(switchboard, with_chainlink, None).await?;

                let mut estiamted_fee = EstimateFee::new(
                    with_switchboard,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );

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

/// Errors when `feeds` contains a Pyth feed, since there is no API key to fetch it with.
fn ensure_no_pyth_feeds(feeds: &TokensWithFeed) -> gmsol_sdk::Result<()> {
    if extract_pyth_feed_ids(feeds)?.is_empty() {
        return Ok(());
    }
    Err(gmsol_sdk::Error::custom(MISSING_PYTH_API_KEY))
}

#[cfg(test)]
mod tests {
    use gmsol_sdk::{
        core::{oracle::PriceProviderKind, token_config::TokenRecord},
        programs::anchor_lang::prelude::Pubkey,
    };

    use super::*;

    fn feeds(providers: &[PriceProviderKind]) -> TokensWithFeed {
        let records = providers
            .iter()
            .map(|provider| TokenRecord::new(Pubkey::new_unique(), Pubkey::new_unique(), *provider))
            .collect();
        TokensWithFeed::try_from_records(records).unwrap()
    }

    #[test]
    fn no_feeds_need_no_key() {
        assert!(ensure_no_pyth_feeds(&feeds(&[])).is_ok());
    }

    #[test]
    fn non_pyth_feeds_need_no_key() {
        let feeds = feeds(&[
            PriceProviderKind::ChainlinkDataStreams,
            PriceProviderKind::Switchboard,
        ]);
        assert!(ensure_no_pyth_feeds(&feeds).is_ok());
    }

    #[test]
    fn pyth_feed_without_key_is_a_config_error() {
        let feeds = feeds(&[
            PriceProviderKind::ChainlinkDataStreams,
            PriceProviderKind::Pyth,
        ]);
        let err = ensure_no_pyth_feeds(&feeds).unwrap_err();
        assert!(err.to_string().contains("PYTH_API_KEY"));
    }
}
