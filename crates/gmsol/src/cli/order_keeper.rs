use std::time::Duration;

use anchor_client::{
    solana_client::rpc_config::RpcSendTransactionConfig, solana_sdk::pubkey::Pubkey,
};
use gmsol::{
    alt::AddressLookupTableOps,
    client::StoreFilter,
    exchange::ExchangeOps,
    solana_utils::transaction_builder::default_before_sign,
    store::glv::GlvOps,
    types::{
        common::ActionHeader, Deposit, DepositCreated, Order, OrderCreated, Withdrawal,
        WithdrawalCreated,
    },
    utils::{instruction::InstructionSerialization, LocalSignerRef, ZeroCopy},
};
use gmsol_model::PositionState;
use tokio::{sync::mpsc::UnboundedSender, time::Instant};

use crate::{
    utils::{Executor, Side},
    GMSOLClient, InstructionBufferCtx,
};

#[derive(clap::Args, Clone)]
pub(super) struct KeeperArgs {
    /// Set the compute unit price in micro lamports.
    #[arg(long, short = 'p', default_value_t = 50_000)]
    compute_unit_price: u64,
    /// The oracle to use.
    #[arg(long, env)]
    oracle: Option<Pubkey>,
    #[cfg_attr(feature = "devnet", arg(long, default_value_t = true))]
    #[cfg_attr(not(feature = "devnet"), arg(long, default_value_t = false))]
    oracle_testnet: bool,
    /// Whether to disable Switchboard support.
    #[arg(long)]
    disable_switchboard: bool,
    /// Feed index.
    #[arg(long, default_value_t = 0)]
    feed_index: u16,
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
    /// Execute.
    Execute { action: Action, address: Pubkey },
}

#[derive(Debug, clap::ValueEnum, Clone, Copy)]
enum Action {
    /// Deposit.
    Deposit,
    /// Withdrawal.
    Withdrawal,
    /// Shift.
    Shift,
    /// Order.
    Order,
    /// GLV deposit.
    GlvDeposit,
    /// GLV withdrawal.
    GlvWithdrawal,
    /// GLV shift.
    GlvShift,
}

impl KeeperArgs {
    fn oracle(&self) -> gmsol::Result<&Pubkey> {
        self.oracle
            .as_ref()
            .ok_or_else(|| gmsol::Error::invalid_argument("oracle is not provided"))
    }

    async fn executor<'a>(
        &'a self,
        client: &'a GMSOLClient,
        store: &Pubkey,
    ) -> gmsol::Result<Executor<'a, LocalSignerRef>> {
        Executor::new_with_envs(
            store,
            client,
            self.oracle_testnet,
            self.feed_index,
            !self.disable_switchboard,
        )
        .await
    }

    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        priority_lamports: u64,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        if serialize_only.is_some() {
            return Err(gmsol::Error::invalid_argument(
                "serialize-only mode is not supported",
            ));
        }
        match &self.command {
            Command::Watch { wait } => {
                crate::utils::instruction_buffer_not_supported(ctx)?;
                let task = Box::pin(self.start_watching(client, store, *wait, priority_lamports));
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
                                    self.with_command(Command::Execute {
                                        action: Action::Deposit,
                                        address: pubkey,
                                    })
                                    .run(
                                        client,
                                        store,
                                        None,
                                        serialize_only,
                                        skip_preflight,
                                        priority_lamports,
                                        max_transaction_size,
                                    ),
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
                                    self.with_command(Command::Execute {
                                        action: Action::Withdrawal,
                                        address: pubkey,
                                    })
                                    .run(
                                        client,
                                        store,
                                        None,
                                        serialize_only,
                                        skip_preflight,
                                        priority_lamports,
                                        max_transaction_size,
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
                                    self.with_command(Command::Execute {
                                        action: Action::Order,
                                        address: pubkey,
                                    })
                                    .run(
                                        client,
                                        store,
                                        None,
                                        serialize_only,
                                        skip_preflight,
                                        priority_lamports,
                                        max_transaction_size,
                                    ),
                                )
                                .await?;
                            }
                        }
                    }
                    kind => {
                        return Err(gmsol::Error::invalid_argument(format!(
                            "Fetching {kind:?} is not supported currently",
                        )));
                    }
                }
            }
            Command::Liquidate { position } => {
                let mut builder = client.liquidate(self.oracle()?, position)?;
                for alt in &self.alts {
                    let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                self.executor(client, store)
                    .await?
                    .execute(
                        builder,
                        ctx,
                        serialize_only,
                        skip_preflight,
                        priority_lamports,
                        max_transaction_size,
                        Some(self.compute_unit_price),
                    )
                    .await?;
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
                self.executor(client, store)
                    .await?
                    .execute(
                        builder,
                        ctx,
                        serialize_only,
                        skip_preflight,
                        priority_lamports,
                        max_transaction_size,
                        Some(self.compute_unit_price),
                    )
                    .await?;
            }
            Command::UpdateAdl { market_token, side } => {
                let builder = client.update_adl(
                    store,
                    self.oracle()?,
                    market_token,
                    side.is_long(),
                    !side.is_long(),
                )?;

                self.executor(client, store)
                    .await?
                    .execute(
                        builder,
                        ctx,
                        serialize_only,
                        skip_preflight,
                        priority_lamports,
                        max_transaction_size,
                        Some(self.compute_unit_price),
                    )
                    .await?;
            }
            Command::CancelOrderIfNoPosition { order, keep } => {
                crate::utils::instruction_buffer_not_supported(ctx)?;
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
                        default_before_sign,
                    )
                    .await?
                    .into_value();
                tracing::info!(%order, "cancelled order at {signature}");
            }
            Command::Execute { action, address } => {
                let executor = self.executor(client, store).await?;

                match action {
                    Action::Deposit => {
                        let builder = client.execute_deposit(store, self.oracle()?, address, true);
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::Withdrawal => {
                        let builder =
                            client.execute_withdrawal(store, self.oracle()?, address, true);
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::Shift => {
                        let builder = client.execute_shift(self.oracle()?, address, true);
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::Order => {
                        let mut builder =
                            client.execute_order(store, self.oracle()?, address, true)?;
                        for alt in &self.alts {
                            let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::GlvDeposit => {
                        let mut builder = client.execute_glv_deposit(self.oracle()?, address, true);
                        for alt in &self.alts {
                            let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::GlvWithdrawal => {
                        let mut builder =
                            client.execute_glv_withdrawal(self.oracle()?, address, true);
                        for alt in &self.alts {
                            let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
                    Action::GlvShift => {
                        let mut builder = client.execute_glv_shift(self.oracle()?, address, true);
                        for alt in &self.alts {
                            let alt = client.alt(alt).await?.ok_or(gmsol::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor
                            .execute(
                                builder,
                                ctx,
                                serialize_only,
                                skip_preflight,
                                priority_lamports,
                                max_transaction_size,
                                Some(self.compute_unit_price),
                            )
                            .await?;
                    }
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
        priority_lamports: u64,
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
                    send_command_after(&tx, event.ts, after, Command::Execute {
                        action: Action::Deposit,
                        address: event.deposit,
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
                    send_command_after(&tx, event.ts, after, Command::Execute {
                        action: Action::Withdrawal,
                        address: event.withdrawal,
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
                    send_command_after(&tx, event.ts, after, Command::Execute {
                        action: Action::Order,
                        address: event.order,
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
                match self
                    .with_command(command)
                    .run(client, &store, None, None, true, priority_lamports, None)
                    .await
                {
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
