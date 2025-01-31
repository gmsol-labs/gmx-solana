use std::time::Duration;

use anchor_client::{
    solana_client::rpc_config::RpcSendTransactionConfig, solana_sdk::pubkey::Pubkey,
};
use futures_util::{FutureExt, TryFutureExt};
use gmsol::{
    alt::AddressLookupTableOps,
    client::StoreFilter,
    exchange::ExchangeOps,
    pyth::{
        pull_oracle::utils::extract_pyth_feed_ids, EncodingType, Hermes, PythPullOracle,
        PythPullOracleContext, PythPullOracleOps,
    },
    store::glv::GlvOps,
    types::{
        common::ActionHeader, Deposit, DepositCreated, Order, OrderCreated, Withdrawal,
        WithdrawalCreated,
    },
    utils::{
        builder::{MakeBundleBuilder, SetExecutionFee},
        instruction::InstructionSerialization,
        ZeroCopy,
    },
};
use gmsol_model::PositionState;
use gmsol_solana_utils::{bundle_builder::SendBundleOptions, compute_budget::ComputeBudget};
use gmsol_store::states::PriceProviderKind;
use tokio::{sync::mpsc::UnboundedSender, time::Instant};

use crate::{utils::Side, GMSOLClient};

#[derive(clap::Args, Clone)]
pub(super) struct KeeperArgs {
    /// Set the compute unit limit.
    #[arg(long, short = 'u')]
    compute_unit_limit: Option<u32>,
    /// Set the compute unit price in micro lamports.
    #[arg(long, short = 'p', default_value_t = 50_000)]
    compute_unit_price: u64,
    /// The oracle to use.
    #[arg(long, env)]
    oracle: Option<Pubkey>,
    /// Set the execution fee to the given instead of estimating one.
    #[arg(long)]
    execution_fee: Option<u64>,
    /// Set the price provider to use.
    #[arg(long, default_value = "pyth")]
    provider: PriceProviderKind,
    /// Whether to use push oracle when available.
    #[arg(long)]
    push_oracle: bool,
    /// ALTs.
    #[arg(long, short = 'a')]
    alts: Vec<Pubkey>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    /// Watch for items creation and execute them.
    Watch {
        #[arg(long, default_value_t = 2)]
        wait: u64,
    },
    /// Execute Deposit.
    ExecuteDeposit { deposit: Pubkey },
    /// Execute Withdrawal.
    ExecuteWithdrawal { withdrawal: Pubkey },
    /// Execute Order.
    ExecuteOrder { order: Pubkey },
    /// Liquidate a position.
    Liquidate { position: Pubkey },
    /// Auto-deleverage a position.
    Adl {
        #[clap(requires = "close_size")]
        position: Pubkey,
        /// The size to be closed.
        #[arg(long, group = "close_size")]
        size: Option<u128>,
        #[arg(long, group = "close_size")]
        close_all: bool,
    },
    /// Update ADL state.
    UpdateAdl {
        market_token: Pubkey,
        #[arg(long, short)]
        side: Side,
    },
    /// Fetch pending actions.
    Pending {
        action: Action,
        #[arg(long, short)]
        verbose: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long)]
        ignore_store: bool,
    },
    /// Cancel order if no position.
    CancelOrderIfNoPosition {
        order: Pubkey,
        #[arg(long)]
        keep: bool,
    },
    /// Execute GLV Deposit.
    ExecuteGlvDeposit { deposit: Pubkey },
    /// Execute GLV Withdrawal.
    ExecuteGlvWithdrawal { withdrawal: Pubkey },
    /// Execute GLV Shift.
    ExecuteGlvShift { shift: Pubkey },
}

#[derive(Debug, clap::ValueEnum, Clone, Copy)]
enum Action {
    /// Deposit.
    Deposit,
    /// Withdrawal.
    Withdrawal,
    /// Order.
    Order,
}

impl KeeperArgs {
    fn get_compute_budget(&self) -> Option<ComputeBudget> {
        let units = self.compute_unit_limit?;
        Some(
            ComputeBudget::default()
                .with_limit(units)
                .with_price(self.compute_unit_price),
        )
    }

    fn use_pyth_pull_oracle(&self) -> bool {
        !self.push_oracle && matches!(self.provider, PriceProviderKind::Pyth)
    }

    fn oracle(&self) -> gmsol::Result<&Pubkey> {
        self.oracle
            .as_ref()
            .ok_or_else(|| gmsol::Error::invalid_argument("oracle is not provided"))
    }

    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: Option<InstructionSerialization>,
    ) -> gmsol::Result<()> {
        if serialize_only.is_some() {
            return Err(gmsol::Error::invalid_argument(
                "serialize-only mode is not supported",
            ));
        }
        match &self.command {
            Command::Watch { wait } => {
                let task = Box::pin(self.start_watching(client, store, *wait));
                task.await?;
            }
            Command::Pending {
                action,
                verbose,
                execute,
                ignore_store,
            } => {
                let store_offset = bytemuck::offset_of!(ActionHeader, store);
                let filter_store = !*ignore_store;
                match action {
                    Action::Deposit => {
                        let actions = client
                            .store_accounts::<ZeroCopy<Deposit>>(
                                filter_store.then(|| {
                                    StoreFilter::new(store, store_offset).ignore_disc_offset(false)
                                }),
                                None,
                            )
                            .await?;
                        if actions.is_empty() {
                            println!("No pending deposits");
                        }
                        for (pubkey, action) in actions {
                            if *verbose {
                                println!("{pubkey}: {action:?}");
                            } else {
                                println!("{pubkey}");
                            }
                            if *execute {
                                Box::pin(
                                    self.with_command(Command::ExecuteDeposit { deposit: pubkey })
                                        .run(client, store, serialize_only),
                                )
                                .await?;
                            }
                        }
                    }
                    Action::Withdrawal => {
                        let actions = client
                            .store_accounts::<ZeroCopy<Withdrawal>>(
                                filter_store.then(|| {
                                    StoreFilter::new(store, store_offset).ignore_disc_offset(false)
                                }),
                                None,
                            )
                            .await?;
                        if actions.is_empty() {
                            println!("No pending withdrawals");
                        }
                        for (pubkey, action) in actions {
                            if *verbose {
                                println!("{pubkey}: {action:?}");
                            } else {
                                println!("{pubkey}");
                            }
                            if *execute {
                                Box::pin(
                                    self.with_command(Command::ExecuteWithdrawal {
                                        withdrawal: pubkey,
                                    })
                                    .run(
                                        client,
                                        store,
                                        serialize_only,
                                    ),
                                )
                                .await?;
                            }
                        }
                    }
                    Action::Order => {
                        let actions = client
                            .store_accounts::<ZeroCopy<Order>>(
                                filter_store.then(|| {
                                    StoreFilter::new(store, store_offset).ignore_disc_offset(false)
                                }),
                                None,
                            )
                            .await?;
                        if actions.is_empty() {
                            println!("No pending orders");
                        }
                        for (pubkey, action) in actions {
                            if *verbose {
                                println!("{pubkey}: {action:?}");
                            } else {
                                println!("{pubkey}");
                            }
                            if *execute {
                                Box::pin(
                                    self.with_command(Command::ExecuteOrder { order: pubkey })
                                        .run(client, store, serialize_only),
                                )
                                .await?;
                            }
                        }
                    }
                }
            }
            Command::ExecuteDeposit { deposit } => {
                let mut builder = client.execute_deposit(store, self.oracle()?, deposit, true);
                let execution_fee = builder.build().await?.estimate_execution_fee(None).await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let feed_ids = extract_pyth_feed_ids(&hint.feeds)?;
                    if feed_ids.is_empty() {
                        tracing::error!(%deposit, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
                        .await?;
                    oracle
                        .execute_with_pyth_price_updates(
                            Some(update.binary()),
                            &mut builder,
                            Some(self.compute_unit_price),
                            true,
                            true,
                        )
                        .await?;
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%deposit, "executed deposit at tx {signatures:#?}");
                    println!("{signatures:#?}");
                }
            }
            Command::ExecuteWithdrawal { withdrawal } => {
                let mut builder =
                    client.execute_withdrawal(store, self.oracle()?, withdrawal, true);
                let execution_fee = self
                    .execution_fee
                    .map(|fee| futures_util::future::ready(Ok(fee)).left_future())
                    .unwrap_or_else(|| {
                        builder
                            .build()
                            .and_then(|builder| async move {
                                Ok(builder
                                    .estimate_execution_fee(Some(self.compute_unit_price))
                                    .await?)
                            })
                            .right_future()
                    })
                    .await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%withdrawal, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let txns = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(txns)
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%withdrawal, "executed withdrawal with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %withdrawal, "failed to execute withdrawal, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%withdrawal, %execution_fee, "executed withdrawal with txs: {signatures:#?}");
                }
            }
            Command::ExecuteOrder { order } => {
                let order_account = client.order(order).await?;
                if let Some(position) = order_account.params().position() {
                    if let Err(gmsol::Error::NotFound) = client.position(position).await {
                        let cancel = client
                            .cancel_order_if_no_position(store, order, Some(position))
                            .await?;
                        let close = client.close_order(order)?.build().await?;
                        let signature = cancel
                            .merge(close)
                            .send_with_options(
                                false,
                                Some(self.compute_unit_price),
                                RpcSendTransactionConfig {
                                    skip_preflight: true,
                                    ..Default::default()
                                },
                            )
                            .await?;
                        tracing::info!(%order, "position does not exist, the order is cancelled");
                        println!("{signature}");
                        return Ok(());
                    }
                }
                let mut builder = client.execute_order(store, self.oracle()?, order, true)?;
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = self
                    .execution_fee
                    .map(|fee| futures_util::future::ready(Ok(fee)).left_future())
                    .unwrap_or_else(|| {
                        builder
                            .build()
                            .and_then(|builder| async move {
                                Ok(builder
                                    .estimate_execution_fee(Some(self.compute_unit_price))
                                    .await?)
                            })
                            .right_future()
                    })
                    .await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%order, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let builder = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(builder)
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%order, %execution_fee, "executed order with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %order, "failed to execute order, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%order, %execution_fee, "executed order with txs: {signatures:#?}");
                }
            }
            Command::Liquidate { position } => {
                let mut builder = client.liquidate(self.oracle()?, position)?;
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = self
                    .execution_fee
                    .map(|fee| futures_util::future::ready(Ok(fee)).left_future())
                    .unwrap_or_else(|| {
                        builder
                            .build()
                            .and_then(|builder| async move {
                                Ok(builder
                                    .estimate_execution_fee(Some(self.compute_unit_price))
                                    .await?)
                            })
                            .right_future()
                    })
                    .await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(hint.feeds())?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%position, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let builder = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(builder)
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%position, %execution_fee, "liquidated position with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %position, "failed to liquidate position, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%position, %execution_fee, "liquidated position with txs: {signatures:#?}");
                }
            }
            Command::Adl {
                position,
                size,
                close_all,
            } => {
                let size = match size {
                    Some(size) => *size,
                    None => {
                        debug_assert!(*close_all);
                        let position = client.position(position).await?;
                        *position.state.size_in_usd()
                    }
                };
                let mut builder = client.auto_deleverage(self.oracle()?, position, size)?;
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = self
                    .execution_fee
                    .map(|fee| futures_util::future::ready(Ok(fee)).left_future())
                    .unwrap_or_else(|| {
                        builder
                            .build()
                            .and_then(|builder| async move {
                                Ok(builder
                                    .estimate_execution_fee(Some(self.compute_unit_price))
                                    .await?)
                            })
                            .right_future()
                    })
                    .await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(hint.feeds())?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%position, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let builder = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(builder)
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%position, %execution_fee, "auto-deleveraged position with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %position, "failed to auto-deleverage position, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%position, %execution_fee, "auto-deleveraged position with txs: {signatures:#?}");
                }
            }
            Command::UpdateAdl { market_token, side } => {
                let mut builder =
                    client.update_adl(store, self.oracle()?, market_token, side.is_long())?;

                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(hint.feeds())?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%market_token, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let builder = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(Some(builder))
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%market_token, ?side, "updated ADL state with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %market_token, ?side, "failed to update ADL state, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .build()
                        .await?
                        .send_with_options(
                            false,
                            compute_unit_price_micro_lamports,
                            Default::default(),
                        )
                        .await?;
                    tracing::info!(%market_token, ?side, "updated ADL state with txs: {signatures:#?}");
                }
            }
            Command::CancelOrderIfNoPosition { order, keep } => {
                let cancel = client
                    .cancel_order_if_no_position(store, order, None)
                    .await?;
                let rpc = if *keep {
                    cancel
                } else {
                    let close = client.close_order(order)?.build().await?;
                    cancel.merge(close)
                };
                let signature = rpc
                    .send_with_options(
                        false,
                        Some(self.compute_unit_price),
                        RpcSendTransactionConfig {
                            skip_preflight: false,
                            ..Default::default()
                        },
                    )
                    .await?;
                tracing::info!(%order, "cancelled order at {signature}");
            }
            Command::ExecuteGlvDeposit { deposit } => {
                let mut builder = client.execute_glv_deposit(self.oracle()?, deposit, true);
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = builder.build().await?.estimate_execution_fee(None).await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let feed_ids = extract_pyth_feed_ids(&hint.feeds)?;
                    if feed_ids.is_empty() {
                        tracing::error!(%deposit, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
                        .await?;
                    oracle
                        .execute_with_pyth_price_updates(
                            Some(update.binary()),
                            &mut builder,
                            Some(self.compute_unit_price),
                            true,
                            true,
                        )
                        .await?;
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%deposit, "executed GLV deposit at tx {signatures:#?}");
                    println!("{signatures:#?}");
                }
            }
            Command::ExecuteGlvWithdrawal { withdrawal } => {
                let mut builder = client.execute_glv_withdrawal(self.oracle()?, withdrawal, true);
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = builder.build().await?.estimate_execution_fee(None).await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let feed_ids = extract_pyth_feed_ids(&hint.feeds)?;
                    if feed_ids.is_empty() {
                        tracing::error!(%withdrawal, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
                        .await?;
                    oracle
                        .execute_with_pyth_price_updates(
                            Some(update.binary()),
                            &mut builder,
                            Some(self.compute_unit_price),
                            true,
                            true,
                        )
                        .await?;
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%withdrawal, "executed GLV withdrawal at tx {signatures:#?}");
                    println!("{signatures:#?}");
                }
            }
            Command::ExecuteGlvShift { shift } => {
                let mut builder = client.execute_glv_shift(self.oracle()?, shift, true);
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                let execution_fee = builder.build().await?.estimate_execution_fee(None).await?;
                builder.set_execution_fee(execution_fee);
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let feed_ids = extract_pyth_feed_ids(&hint.feeds)?;
                    if feed_ids.is_empty() {
                        tracing::error!(%shift, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client)?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(&feed_ids, Some(EncodingType::Base64))
                        .await?;
                    oracle
                        .execute_with_pyth_price_updates(
                            Some(update.binary()),
                            &mut builder,
                            Some(self.compute_unit_price),
                            true,
                            true,
                        )
                        .await?;
                } else {
                    let builder = builder.build().await?;
                    let compute_unit_price_micro_lamports =
                        self.get_compute_budget().map(|budget| budget.price());
                    let signatures = builder
                        .send_all_with_opts(SendBundleOptions {
                            compute_unit_price_micro_lamports,
                            ..Default::default()
                        })
                        .await?;
                    tracing::info!(%shift, "executed GLV shift at tx {signatures:#?}");
                    println!("{signatures:#?}");
                }
            }
        }
        Ok(())
    }

    fn with_command(&self, command: Command) -> Self {
        let mut args = self.clone();
        args.command = command;
        args
    }

    async fn start_watching(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        wait: u64,
    ) -> gmsol::Result<()> {
        use tokio::sync::mpsc;

        let store = *store;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut unsubscribers = vec![];

        let after = Duration::from_secs(wait);
        // Subscribe deposit creation event.
        let deposit_program = client.anchor().program(*client.store_program_id())?;
        let unsubscriber =
            deposit_program
            .on::<DepositCreated>({
                let tx = tx.clone();
                move |ctx, event| {
                if event.store == store {
                    tracing::info!(slot=%ctx.slot, ?event, "received a new deposit creation event");
                    send_command_after(&tx, event.ts, after, Command::ExecuteDeposit {
                        deposit: event.deposit,
                    });
                } else {
                    tracing::debug!(slot=%ctx.slot, ?event, "received deposit creation event from other store");
                }
            }})
            .await?;
        unsubscribers.push(unsubscriber);

        tracing::info!("deposit creation subscribed");

        // Subscribe withdrawal creation event.
        let withdrawal_program = client.anchor().program(*client.store_program_id())?;
        let unsubscriber = withdrawal_program
            .on::<WithdrawalCreated>({
                let tx = tx.clone();
                move |ctx, event| {
                if event.store == store {
                    tracing::info!(slot=%ctx.slot, ?event, "received a new withdrawal creation event");
                    send_command_after(&tx, event.ts, after, Command::ExecuteWithdrawal {
                        withdrawal: event.withdrawal,
                    });
                } else {
                    tracing::debug!(slot=%ctx.slot, ?event, "received withdrawal creation event from other store");
                }
            }})
            .await?;
        unsubscribers.push(unsubscriber);

        tracing::info!("withdrawal creation subscribed");

        // Subscribe order creation event.
        let order_program = client.anchor().program(*client.store_program_id())?;
        let unsubscriber = order_program
            .on::<OrderCreated>({
                let tx = tx.clone();
                move |ctx, event| {
                if event.store == store {
                    tracing::info!(slot=%ctx.slot, ?event, "received a new order creation event");
                    send_command_after(&tx, event.ts, after, Command::ExecuteOrder {
                        order: event.order,
                    });
                } else {
                    tracing::debug!(slot=%ctx.slot, ?event, "received order creation event from other store");
                }
            }})
            .await?;
        unsubscribers.push(unsubscriber);

        tracing::info!("order creation subscribed");

        let worker = async move {
            while let Some(command) = rx.recv().await {
                tracing::info!(?command, "received new command");
                match self.with_command(command).run(client, &store, None).await {
                    Ok(()) => {
                        tracing::info!("command executed");
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to execute, ignore");
                    }
                }
            }
            gmsol::Result::Ok(())
        };
        tokio::select! {
            res = tokio::signal::ctrl_c() => {
                match res {
                    Ok(()) => {
                        tracing::info!("Received `ctrl + c`, stopping...");
                    },
                    Err(err) => {
                        tracing::error!(%err, "Failed to setup signal handler");
                    }
                }

            },
            res = worker => {
                res?;
            }
        }
        for unsubscriber in unsubscribers {
            unsubscriber.unsubscribe().await;
        }
        Ok(())
    }
}

fn send_command_until(tx: &UnboundedSender<Command>, deadline: Instant, command: Command) {
    use tokio::time::sleep_until;

    tokio::spawn({
        let tx = tx.clone();
        async move {
            sleep_until(deadline).await;
            tx.send(command).unwrap();
        }
    });
}

fn send_command_after(tx: &UnboundedSender<Command>, ts: i64, after: Duration, command: Command) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let wait = Duration::from_secs(ts as u64)
        .saturating_add(after)
        .saturating_sub(SystemTime::now().duration_since(UNIX_EPOCH).unwrap());
    let deadline = Instant::now().checked_add(wait).unwrap();
    send_command_until(tx, deadline, command);
}
