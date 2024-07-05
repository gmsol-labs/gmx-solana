use std::{ops::Deref, time::Duration};

use anchor_client::{
    anchor_lang::{AccountDeserialize, Discriminator},
    solana_client::rpc_filter::{Memcmp, RpcFilterType},
    solana_sdk::{pubkey::Pubkey, signer::Signer},
    Program,
};
use futures_util::{FutureExt, TryFutureExt};
use gmsol::{
    exchange::ExchangeOps,
    pyth::{EncodingType, Hermes, PythPullOracle, PythPullOracleContext, PythPullOracleOps},
    types::{Deposit, Order, Withdrawal},
    utils::{ComputeBudget, RpcBuilder},
};
use gmsol_exchange::events::{DepositCreatedEvent, OrderCreatedEvent, WithdrawalCreatedEvent};
use gmsol_store::states::PriceProviderKind;
use tokio::{sync::mpsc::UnboundedSender, time::Instant};

use crate::{utils::Oracle, GMSOLClient};

#[derive(clap::Args, Clone)]
pub(super) struct KeeperArgs {
    /// Set the compute unit limit.
    #[arg(long, short = 'u')]
    compute_unit_limit: Option<u32>,
    /// Set the compute unit price in micro lamports.
    #[arg(long, short = 'p', default_value_t = 50_000)]
    compute_unit_price: u64,
    /// The oracle to use.
    #[command(flatten)]
    oracle: Oracle,
    /// Set the execution fee to the given instead of estimating one.
    #[arg(long)]
    execution_fee: Option<u64>,
    /// Set the price provider to use.
    #[arg(long, default_value = "pyth")]
    provider: PriceProviderKind,
    /// Whether to use pull oracle when available.
    #[arg(long)]
    pull_oracle: bool,
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

    async fn get_or_estimate_execution_fee<S, C>(
        &self,
        program: &Program<C>,
        mut rpc: RpcBuilder<'_, C>,
    ) -> gmsol::Result<u64>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        if let Some(fee) = self.execution_fee {
            return Ok(fee);
        }
        if let Some(budget) = self.get_compute_budget() {
            rpc = rpc.compute_budget(budget);
        } else {
            rpc.compute_budget_mut().set_price(self.compute_unit_price);
        }
        rpc.estimate_execution_fee(&program.async_rpc(), None).await
    }

    fn use_pyth_pull_oracle(&self) -> bool {
        self.pull_oracle && matches!(self.provider, PriceProviderKind::Pyth)
    }

    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
    ) -> gmsol::Result<()> {
        if serialize_only {
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
                let filter_store = !*ignore_store;
                match action {
                    Action::Deposit => {
                        let actions = self
                            .fetch_pendings::<Deposit>(client, store, 8, filter_store)
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
                        let actions = self
                            .fetch_pendings::<Withdrawal>(client, store, 8, filter_store)
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
                        let actions = self
                            .fetch_pendings::<Order>(client, store, 9, filter_store)
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
                let mut builder = client.execute_deposit(
                    store,
                    &self
                        .oracle
                        .address(Some(store), &client.data_store_program_id())?,
                    deposit,
                    true,
                );
                let execution_fee = self
                    .get_or_estimate_execution_fee(client.data_store(), builder.build().await?)
                    .await?;
                builder
                    .execution_fee(execution_fee)
                    .price_provider(self.provider.program());
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%deposit, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client.anchor())?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let rpc = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(Some(rpc))
                        })
                        .await?;
                    match with_prices
                        .send_all(Some(self.compute_unit_price), true)
                        .await
                    {
                        Ok(signatures) => {
                            tracing::info!(%deposit, "executed deposit with txs {signatures:#?}");
                        }
                        Err((signatures, err)) => {
                            tracing::error!(%err, %deposit, "failed to execute deposit, successful txs: {signatures:#?}");
                        }
                    }
                } else {
                    let mut rpc = builder.build().await?;
                    if let Some(budget) = self.get_compute_budget() {
                        rpc = rpc.compute_budget(budget)
                    }
                    let signature = rpc.build().send().await?;
                    tracing::info!(%deposit, "executed deposit at tx {signature}");
                    println!("{signature}");
                }
            }
            Command::ExecuteWithdrawal { withdrawal } => {
                let mut builder = client.execute_withdrawal(
                    store,
                    &self
                        .oracle
                        .address(Some(store), &client.data_store_program_id())?,
                    withdrawal,
                    true,
                );
                let execution_fee = self
                    .get_or_estimate_execution_fee(client.data_store(), builder.build().await?)
                    .await?;
                builder
                    .execution_fee(execution_fee)
                    .price_provider(self.provider.program());
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%withdrawal, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client.anchor())?;
                    let hermes = Hermes::default();
                    let update = hermes
                        .latest_price_updates(feed_ids, Some(EncodingType::Base64))
                        .await?;
                    let with_prices = oracle
                        .with_pyth_prices(&ctx, &update, |prices| async {
                            let rpc = builder
                                .parse_with_pyth_price_updates(prices)
                                .build()
                                .await?;
                            Ok(Some(rpc))
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
                    let mut rpc = builder.build().await?;
                    if let Some(budget) = self.get_compute_budget() {
                        rpc = rpc.compute_budget(budget)
                    }
                    let signature = rpc.build().send().await?;
                    tracing::info!(%withdrawal, "executed withdrawal at tx {signature}");
                    println!("{signature}");
                }
            }
            Command::ExecuteOrder { order } => {
                let mut builder = client.execute_order(
                    store,
                    &self
                        .oracle
                        .address(Some(store), &client.data_store_program_id())?,
                    order,
                    true,
                )?;
                let execution_fee = self
                    .execution_fee
                    .map(|fee| futures_util::future::ready(Ok(fee)).left_future())
                    .unwrap_or_else(|| {
                        builder
                            .build()
                            .and_then(|builder| async move {
                                builder
                                    .estimated_execution_fee(Some(self.compute_unit_price))
                                    .await
                            })
                            .right_future()
                    })
                    .await?;
                builder
                    .execution_fee(execution_fee)
                    .price_provider(self.provider.program());
                if self.use_pyth_pull_oracle() {
                    let hint = builder.prepare_hint().await?;
                    let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
                    let feed_ids = ctx.feed_ids();
                    if feed_ids.is_empty() {
                        tracing::error!(%order, "empty feed ids");
                    }
                    let oracle = PythPullOracle::try_new(client.anchor())?;
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
                        .send_all_with_opts(
                            compute_unit_price_micro_lamports,
                            Default::default(),
                            false,
                        )
                        .await?;
                    tracing::info!(%order, %execution_fee, "executed order with txs: {signatures:#?}");
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

    async fn fetch_pendings<T>(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        store_offset: usize,
        filter_store: bool,
    ) -> gmsol::Result<Vec<(Pubkey, T)>>
    where
        T: AccountDeserialize + Discriminator,
    {
        let filters = std::iter::empty().chain(filter_store.then(|| {
            tracing::debug!(%store, "store bytes to filter: {}", hex::encode(store));
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                store_offset,
                store.as_ref().to_owned(),
            ))
        }));
        client
            .data_store()
            .accounts_lazy::<T>(filters.collect())
            .await?
            .map(|res| res.map_err(gmsol::Error::from))
            .collect()
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
        let deposit_program = client.new_exchange()?;
        let unsubscriber =
            deposit_program
            .on::<DepositCreatedEvent>({
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
        let withdrawal_program = client.new_exchange()?;
        let unsubscriber = withdrawal_program
            .on::<WithdrawalCreatedEvent>({
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
        let order_program = client.new_exchange()?;
        let unsubscriber = order_program
            .on::<OrderCreatedEvent>({
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
                match self.with_command(command).run(client, &store, false).await {
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
