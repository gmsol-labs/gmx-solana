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
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        bundle_builder::BundleOptions,
        client::wait_for_blockhash_expiry,
        make_bundle_builder::{EstimateFee, MakeBundleBuilder, SetExecutionFee},
        signer::LocalSignerRef,
        solana_sdk::hash::Hash,
    },
};

use crate::{
    commands::display_signatures,
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

        // Borrowed (not moved) below so its temporary accounts can be read after sending.
        let mut with_pyth = WithPullOracle::new(pyth, consumer, None).await?;

        let bundle = match (chainlink.as_ref(), switchboard) {
            (None, None) => {
                let mut estiamted_fee = EstimateFee::new(
                    &mut with_pyth,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), None) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), &mut with_pyth, None).await?;

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
                let with_switchboard =
                    WithPullOracle::new(switchboard, &mut with_pyth, None).await?;

                let mut estiamted_fee = EstimateFee::new(
                    with_switchboard,
                    compute_unit_price,
                    compute_unit_min_priority_lamports,
                );
                estiamted_fee.build_with_options(options).await?
            }
            (Some(chainlink), Some(switchboard)) => {
                let (with_chainlink, feed_ids) =
                    WithPullOracle::from_consumer(chainlink.clone(), &mut with_pyth, None).await?;

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

        // Sign with a blockhash fetched here, so an unresolved send can be waited out exactly
        // until its last valid block height.
        let blockhash = self.bundle_blockhash().await?;
        let mut landed = false;
        let sent = self
            .client
            .send_or_serialize_with_blockhash(
                bundle,
                blockhash.map(|(hash, _)| hash),
                |signatures, err, steps| {
                    landed = !signatures.is_empty();
                    display_signatures(signatures, err, steps)
                },
            )
            .await;
        if let (Err(err), Some((_, last_valid_block_height))) = (&sent, blockhash) {
            if needs_cleanup(landed, err) {
                self.close_temporary_accounts(
                    with_pyth.temporary_accounts(),
                    err,
                    last_valid_block_height,
                )
                .await;
            }
        }
        sent
    }

    /// The blockhash to sign a directly sent bundle with, and its last valid block height.
    /// `None` when the bundle is only serialized or written to an instruction buffer.
    async fn bundle_blockhash(&self) -> gmsol_sdk::Result<Option<(Hash, u64)>> {
        if !self.client.sends_directly() {
            return Ok(None);
        }
        let rpc = self.client.rpc();
        let blockhash = rpc
            .get_latest_blockhash_with_commitment(rpc.commitment())
            .await
            .map_err(gmsol_sdk::Error::custom)?;
        Ok(Some(blockhash))
    }

    /// Close the temporary Pyth accounts left open when an execute bundle stopped before its
    /// close batch. Never fails: every account not confirmed closed is printed to stderr.
    async fn close_temporary_accounts(
        &self,
        accounts: &[Pubkey],
        send_err: &gmsol_sdk::Error,
        last_valid_block_height: u64,
    ) {
        if accounts.is_empty() {
            return;
        }
        let still_open = match self
            .try_close_temporary_accounts(accounts, send_err, last_valid_block_height)
            .await
        {
            Ok(still_open) => still_open,
            Err(err) => {
                // The state of every account is unknown, so report them all.
                eprintln!("\nFailed to close temporary Pyth accounts: {err}");
                accounts.to_vec()
            }
        };
        if still_open.is_empty() {
            return;
        }
        // `eprintln!`, not `warn!`: this output is the only record of the addresses, so it
        // must show whatever the log level is.
        eprintln!(
            "Temporary Pyth accounts still open; their rent stays locked until they are closed. \
             Retry closing them later:"
        );
        for account in &still_open {
            eprintln!("  {account}");
        }
    }

    /// Returns the accounts that are still open after the cleanup, read at the client's
    /// commitment level.
    async fn try_close_temporary_accounts(
        &self,
        accounts: &[Pubkey],
        send_err: &gmsol_sdk::Error,
        last_valid_block_height: u64,
    ) -> gmsol_sdk::Result<Vec<Pubkey>> {
        // A transaction with an unknown outcome may still create an account; wait until it
        // cannot land, or that account would be missed.
        if is_unresolved(send_err) {
            // `println!`, not `info!`: the default log level hides info, and the wait can
            // take minutes, which would look like a hang.
            println!("\nWaiting for unresolved transactions to expire before closing temporary Pyth accounts (up to ~3 min)...");
            // Wait at the client's own commitment: the lookup below reads at that level, so
            // a stricter level there (e.g. `finalized`) must not lag behind the wait.
            let rpc = self.client.rpc();
            wait_for_blockhash_expiry(rpc, last_valid_block_height, rpc.commitment()).await?;
        }
        let bundle = self
            .pyth
            .close_temporary_accounts(self.client, accounts)
            .await?;
        if bundle.is_empty() {
            let commitment = self.client.rpc().commitment().commitment;
            println!(
                "\nNo temporary Pyth accounts found on-chain at {commitment}; nothing to close. Checked:"
            );
            for account in accounts {
                println!("  {account}");
            }
            return Ok(vec![]);
        }
        println!("\nClosing temporary Pyth accounts left open by the failed execute:");
        if let Err(err) = self.client.send_or_serialize(bundle).await {
            // Not fatal: the lookup below reports exactly what is still open.
            eprintln!("Closing failed: {err}");
        }
        self.pyth
            .open_temporary_accounts(self.client, accounts)
            .await
    }
}

/// `true` when a failed execute bundle may have left temporary accounts open: some
/// transaction landed, or one has an unknown outcome. When neither holds, nothing was created
/// (this also covers errors raised before anything was sent), so there is nothing to close.
fn needs_cleanup(landed: bool, send_err: &gmsol_sdk::Error) -> bool {
    landed || is_unresolved(send_err)
}

/// `true` when some transaction of the bundle has an unknown outcome.
///
/// `send_or_serialize` reports a compressed error, which keeps a `SendUnresolved` over any
/// later failure, so this sees every bundle that had one.
fn is_unresolved(err: &gmsol_sdk::Error) -> bool {
    matches!(err, gmsol_sdk::Error::SolanaUtils(err) if err.unresolved_signature().is_some())
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

    fn unresolved_error() -> gmsol_sdk::Error {
        use gmsol_sdk::solana_utils::{solana_sdk::signature::Signature, Error as UtilsError};

        let timed_out = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        UtilsError::SendUnresolved {
            signature: Signature::default(),
            source: Box::new(timed_out.into()),
        }
        .into()
    }

    #[test]
    fn cleanup_when_a_transaction_landed() {
        let on_chain_failure = gmsol_sdk::Error::custom("execute failed on-chain");
        assert!(needs_cleanup(true, &on_chain_failure));
    }

    #[test]
    fn cleanup_when_an_outcome_is_unknown() {
        assert!(needs_cleanup(false, &unresolved_error()));
    }

    #[test]
    fn no_cleanup_when_nothing_was_created() {
        // e.g. the first setup transaction failed, or a lookup table fetch failed before sending.
        let failure = gmsol_sdk::Error::custom("rejected before landing");
        assert!(!needs_cleanup(false, &failure));
    }

    #[test]
    fn only_send_unresolved_counts_as_unresolved() {
        use gmsol_sdk::solana_utils::Error as UtilsError;

        assert!(is_unresolved(&unresolved_error()));
        assert!(!is_unresolved(
            &UtilsError::custom("on-chain failure").into()
        ));
        assert!(!is_unresolved(&gmsol_sdk::Error::custom("no price")));
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
