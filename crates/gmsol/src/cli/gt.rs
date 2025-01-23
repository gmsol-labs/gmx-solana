use std::num::NonZeroU32;

use crate::{
    utils::{send_or_serialize_rpc, table_format},
    GMSOLClient, TimelockCtx,
};
use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{
    store::gt::GtOps,
    treasury::TreasuryOps,
    utils::{instruction::InstructionSerialization, unsigned_amount_to_decimal},
};
use prettytable::{row, Table};
use rust_decimal::Decimal;

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(long)]
    debug: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Status.
    Status,
    /// Balance.
    Balance {
        #[arg(long, group = "balance-input")]
        owner: Option<Pubkey>,
        /// Confirm the operation.
        #[arg(long)]
        confirm: bool,
    },
    /// Prepare GT exchange vault.
    PrepareExchangeVault,
    /// Confirm the given GT exchange vault.
    ConfirmExchangeVault {
        address: Pubkey,
        /// Whether to skip the initialization of current exchange vault.
        #[arg(long)]
        skip_init_current: bool,
    },
    /// Set GT exchange time window.
    SetExchangeTimeWindow { seconds: NonZeroU32 },
    /// Get or request GT exchange.
    Exchange {
        #[arg(
            long,
            value_name = "AMOUNT",
            group = "exchange-input",
            requires = "confirm"
        )]
        request: Option<String>,
        #[arg(
            long,
            value_name = "ADDRESS",
            group = "exchange-input",
            requires = "confirm"
        )]
        complete: Option<Pubkey>,
        #[arg(long, group = "exchange-input")]
        owner: Option<Pubkey>,
        /// Confirm the operation.
        #[arg(long)]
        confirm: bool,
        /// Whether to prepare the exchange vault before the request or not.
        #[arg(long, requires = "request")]
        prepare_vault: bool,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        timelock: Option<TimelockCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::Status => {
                let store = client.store(store).await?;
                let gt = store.gt();
                let decimals = gt.decimals();

                println!(
                    "Total Minted: {}",
                    unsigned_amount_to_decimal(gt.total_minted(), decimals).normalize()
                );
                println!(
                    "GT Supply: {}",
                    unsigned_amount_to_decimal(gt.supply(), decimals).normalize()
                );
                println!(
                    "GT Vault: {}",
                    unsigned_amount_to_decimal(gt.gt_vault(), decimals).normalize()
                );
            }
            Command::Balance { owner, confirm: _ } => {
                let owner = owner.unwrap_or(client.payer());
                let user = client.find_user_address(store, &owner);
                let user = client.user(&user).await?;
                let store_account = client.store(store).await?;
                let decimals = store_account.gt().decimals();

                let gt = user.gt().amount();
                let rank = user.gt().rank();

                println!(
                    "GT: {}",
                    unsigned_amount_to_decimal(gt, decimals).normalize()
                );
                println!("User Rank: {rank}");
            }
            Command::PrepareExchangeVault => {
                let time_window = client.store(store).await?.gt().exchange_time_window();
                let (rpc, _vault) = client
                    .prepare_gt_exchange_vault_with_time_window(store, time_window)?
                    .swap_output(());
                send_or_serialize_rpc(
                    store,
                    rpc,
                    timelock,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::ConfirmExchangeVault {
                address,
                skip_init_current,
            } => {
                let time_window = client.store(store).await?.gt().exchange_time_window();

                let init = (!*skip_init_current)
                    .then(|| client.prepare_gt_exchange_vault_with_time_window(store, time_window))
                    .transpose()?
                    .map(|rpc| rpc.with_output(()));

                let mut rpc = client.confirm_gt_exchange_vault(store, address);

                if let Some(init) = init {
                    rpc = rpc.merge(init);
                }

                send_or_serialize_rpc(
                    store,
                    rpc,
                    timelock,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::SetExchangeTimeWindow { seconds } => {
                let rpc = client.gt_set_exchange_time_window(store, seconds.get());
                send_or_serialize_rpc(
                    store,
                    rpc,
                    timelock,
                    serialize_only,
                    skip_preflight,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::Exchange {
                request: amount,
                complete,
                owner,
                confirm: _,
                prepare_vault,
            } => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt().exchange_time_window();
                let decimals = store_account.gt().decimals();

                match (amount, complete) {
                    (Some(amount), None) => {
                        let amount = parse_amount(amount, decimals)?;
                        let request = client.request_gt_exchange_with_time_window(
                            store,
                            time_window,
                            amount,
                        )?;
                        let rpc = if *prepare_vault {
                            let prepare = client
                                .prepare_gt_exchange_vault_with_time_window(store, time_window)?
                                .with_output(());
                            prepare.merge(request)
                        } else {
                            request
                        };
                        send_or_serialize_rpc(
                            store,
                            rpc,
                            timelock,
                            serialize_only,
                            skip_preflight,
                            |signature| {
                                println!("{signature}");
                                Ok(())
                            },
                        )
                        .await?;
                    }
                    (None, Some(exchange)) => {
                        let rpc = client
                            .complete_gt_exchange(store, exchange, None, None, None)
                            .await?;
                        send_or_serialize_rpc(
                            store,
                            rpc,
                            timelock,
                            serialize_only,
                            skip_preflight,
                            |signature| {
                                println!("{signature}");
                                Ok(())
                            },
                        )
                        .await?;
                    }
                    (None, None) => {
                        let owner = owner.unwrap_or(client.payer());
                        let exchanges = client.gt_exchanges(store, &owner).await?;

                        let mut table = Table::new();
                        table.set_titles(row!["Pubkey", "Vault", "Amount",]);
                        table.set_format(table_format());

                        for (address, exchange) in exchanges {
                            let amount =
                                unsigned_amount_to_decimal(exchange.amount(), decimals).normalize();
                            table.add_row(row![address, exchange.vault(), amount]);
                        }

                        println!("{table}");
                    }
                    _ => {
                        unreachable!()
                    }
                }
            }
        }
        Ok(())
    }
}

fn parse_amount(amount: &str, decimals: u8) -> gmsol::Result<u64> {
    let mut amount: Decimal = amount.parse().map_err(gmsol::Error::unknown)?;
    amount.rescale(decimals as u32);
    let amount = amount
        .mantissa()
        .try_into()
        .map_err(gmsol::Error::invalid_argument)?;
    Ok(amount)
}
