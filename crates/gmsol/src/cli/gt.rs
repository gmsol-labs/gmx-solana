use std::num::NonZeroU32;

use crate::{
    utils::{send_or_serialize_rpc, table_format},
    GMSOLClient,
};
use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{
    store::gt::GtOps,
    types::gt::GtVesting,
    utils::{unsigned_amount_to_decimal, ZeroCopy},
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
        /// Claim pending esGT.
        #[arg(long, group = "balance-input", requires = "confirm")]
        claim: bool,
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
    /// Set esGT vault receiver.
    SetReceiver { address: Pubkey },
    /// Set esGT receiver factor.
    SetReceiverFactor { factor: u128 },
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
        close: Option<Pubkey>,
        #[arg(long, group = "exchange-input")]
        owner: Option<Pubkey>,
        /// Confirm the operation.
        #[arg(long)]
        confirm: bool,
        /// Whether to prepare the exchange vault before the request or not.
        #[arg(long, requires = "request")]
        prepare_vault: bool,
    },
    /// Vest esGT.
    Vest {
        #[arg(
            long,
            value_name = "AMOUNT",
            group = "vest-input",
            requires = "confirm"
        )]
        request: Option<String>,
        #[arg(long, group = "vest-input", requires = "confirm")]
        claim: bool,
        #[arg(
            long,
            value_name = "AMOUNT",
            group = "vest-input",
            requires = "confirm"
        )]
        from_vault: Option<String>,
        #[arg(long, group = "vest-input")]
        owner: Option<Pubkey>,
        /// Confirm the operation.
        #[arg(long)]
        confirm: bool,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: bool,
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
                    "esGT Supply: {}",
                    unsigned_amount_to_decimal(gt.es_supply(), decimals).normalize()
                );
                println!(
                    "esGT Receiver Vault: {}",
                    unsigned_amount_to_decimal(gt.es_vault(), decimals).normalize()
                );
            }
            Command::Balance {
                owner,
                claim,
                confirm: _,
            } => {
                use gmsol_model::utils::apply_factor;

                if *claim {
                    let rpc = client.claim_es_gt(store);
                    send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                        println!("{signature}");
                        Ok(())
                    })
                    .await?;
                } else {
                    let owner = owner.unwrap_or(client.payer());
                    let user = client.find_user_address(store, &owner);
                    let user = client.user(&user).await?;
                    let store_account = client.store(store).await?;
                    let decimals = store_account.gt().decimals();

                    let gt = user.gt().amount();
                    let es_gt = user.gt().es_amount();
                    let vesting_es_gt = user.gt().vesting_es_amount();
                    let rank = user.gt().rank();

                    let factor = store_account.gt().es_factor();
                    let user_factor = user.gt().es_factor();
                    let diff_factor = factor.saturating_sub(user_factor);

                    let pending_es_gt: u64 =
                        apply_factor::<_, { gmsol::constants::MARKET_DECIMALS }>(
                            &(gt as u128),
                            &diff_factor,
                        )
                        .ok_or_else(|| {
                            gmsol::Error::unknown("calculating pending esGT amount overflow")
                        })?
                        .try_into()
                        .map_err(|_| {
                            gmsol::Error::unknown("failed to converting the result into amount")
                        })?;

                    println!(
                        "GT: {}",
                        unsigned_amount_to_decimal(gt, decimals).normalize()
                    );
                    println!(
                        "esGT: {}",
                        unsigned_amount_to_decimal(es_gt, decimals).normalize()
                    );
                    println!(
                        "Pending esGT: {}",
                        unsigned_amount_to_decimal(pending_es_gt, decimals).normalize()
                    );
                    println!(
                        "Vesting esGT: {}",
                        unsigned_amount_to_decimal(vesting_es_gt, decimals).normalize()
                    );
                    println!("User Rank: {rank}");
                }
            }
            Command::PrepareExchangeVault => {
                let time_window = client.store(store).await?.gt().exchange_time_window();
                let (rpc, _vault) = client
                    .prepare_gt_exchange_vault_with_time_window(store, time_window)?
                    .swap_output(());
                send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                    println!("{signature}");
                    Ok(())
                })
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

                send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                    println!("{signature}");
                    Ok(())
                })
                .await?;
            }
            Command::SetExchangeTimeWindow { seconds } => {
                let rpc = client.gt_set_exchange_time_window(store, seconds.get());
                send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                    println!("{signature}");
                    Ok(())
                })
                .await?;
            }
            Command::SetReceiver { address } => {
                let rpc = client.gt_set_es_receiver(store, address);
                send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                    println!("{signature}");
                    Ok(())
                })
                .await?;
            }
            Command::SetReceiverFactor { factor } => {
                let rpc = client.gt_set_es_receiver_factor(store, *factor);
                send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                    println!("{signature}");
                    Ok(())
                })
                .await?;
            }
            Command::Exchange {
                request: amount,
                close,
                owner,
                confirm: _,
                prepare_vault,
            } => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt().exchange_time_window();
                let decimals = store_account.gt().decimals();

                match (amount, close) {
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
                        send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                            println!("{signature}");
                            Ok(())
                        })
                        .await?;
                    }
                    (None, Some(exchange)) => {
                        let rpc = client
                            .close_gt_exchange(store, exchange, None, None)
                            .await?;
                        send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                            println!("{signature}");
                            Ok(())
                        })
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
            Command::Vest {
                request,
                claim,
                from_vault,
                owner,
                confirm: _,
            } => {
                let store_account = client.store(store).await?;
                let decimals = store_account.gt().decimals();

                if *claim {
                    let rpc = client.update_gt_vesting(store);
                    send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                        println!("{signature}");
                        Ok(())
                    })
                    .await?;
                } else if let Some(amount) = from_vault {
                    let amount = parse_amount(amount, decimals)?;
                    let rpc = client.claim_es_vesting_from_vault(store, amount);
                    send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                        println!("{signature}");
                        Ok(())
                    })
                    .await?;
                } else if let Some(amount) = request {
                    let amount = parse_amount(amount, decimals)?;
                    let rpc = client.request_gt_vesting(store, amount);
                    send_or_serialize_rpc(rpc, serialize_only, skip_preflight, |signature| {
                        println!("{signature}");
                        Ok(())
                    })
                    .await?;
                } else {
                    let owner = owner.unwrap_or(client.payer());
                    let vesting = client.find_gt_vesting_address(store, &owner);
                    let vesting = client
                        .account::<ZeroCopy<GtVesting>>(&vesting)
                        .await?
                        .ok_or(gmsol::Error::NotFound)?
                        .0;
                    if self.debug {
                        println!("{vesting:?}");
                    } else {
                        let total_vesting: u64 = vesting.vesting().map(|amount| amount.get()).sum();
                        let claimable = vesting.claimable(current_ts()?);
                        println!(
                            "Total Vesting: {}",
                            unsigned_amount_to_decimal(total_vesting, decimals).normalize()
                        );
                        println!(
                            "Claimable: {}",
                            unsigned_amount_to_decimal(claimable, decimals).normalize()
                        )
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

fn current_ts() -> gmsol::Result<i64> {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(gmsol::Error::unknown)?;
    Ok(now.as_secs() as i64)
}
