use crate::config::DisplayOptions;
use gmsol_sdk::{
    ops::{gt::GtOps, treasury::TreasuryOps},
    programs::anchor_lang::prelude::Pubkey,
    serde::StringPubkey,
    utils::{unsigned_amount_to_decimal, Amount},
};
use std::num::NonZeroU32;

/// GT-related commands.
#[derive(Debug, clap::Args)]
pub struct Gt {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Status.
    Status {
        #[arg(long)]
        debug: bool,
    },
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
        request: Option<Amount>,
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

impl super::Command for Gt {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let txn = match &self.command {
            Command::Status { debug } => {
                let store_account = client.store(store).await?;
                let gt = store_account.gt;

                if *debug {
                    println!("{gt:#?}");
                }

                let decimals = gt.decimals;

                println!(
                    "Total Minted: {}",
                    unsigned_amount_to_decimal(gt.total_minted, decimals).normalize()
                );
                println!(
                    "GT Supply: {}",
                    unsigned_amount_to_decimal(gt.supply, decimals).normalize()
                );
                println!(
                    "GT Vault: {}",
                    unsigned_amount_to_decimal(gt.gt_vault, decimals).normalize()
                );
                return Ok(());
            }
            Command::Balance { owner, confirm: _ } => {
                let owner = owner.unwrap_or(client.payer());
                let user = client.find_user_address(store, &owner);
                let user = client.user(&user).await?;
                let store_account = client.store(store).await?;
                let decimals = store_account.gt.decimals;

                let gt = user.gt.amount;
                let rank = user.gt.rank;

                println!(
                    "GT: {}",
                    unsigned_amount_to_decimal(gt, decimals).normalize()
                );
                println!("User Rank: {rank}");
                return Ok(());
            }
            Command::PrepareExchangeVault => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt.exchange_time_window;
                let (rpc, _vault) = client
                    .prepare_gt_exchange_vault_with_time_window(store, time_window)?
                    .swap_output(());
                rpc
            }
            Command::ConfirmExchangeVault {
                address,
                skip_init_current,
            } => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt.exchange_time_window;

                let init = (!*skip_init_current)
                    .then(|| client.prepare_gt_exchange_vault_with_time_window(store, time_window))
                    .transpose()?
                    .map(|rpc| rpc.output(()));

                let mut rpc = client.confirm_gt_exchange_vault(store, address);

                if let Some(init) = init {
                    rpc = rpc.merge(init);
                }

                rpc
            }
            Command::SetExchangeTimeWindow { seconds } => {
                client.gt_set_exchange_time_window(store, seconds.get())
            }
            Command::Exchange {
                request: amount,
                complete,
                owner,
                confirm: _,
                prepare_vault,
            } => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt.exchange_time_window;
                let decimals = store_account.gt.decimals;

                match (amount, complete) {
                    (Some(amount), None) => {
                        let amount = amount.to_u64(decimals)?;
                        let request = client.request_gt_exchange_with_time_window(
                            store,
                            time_window,
                            amount,
                        )?;
                        let rpc = if *prepare_vault {
                            let prepare = client
                                .prepare_gt_exchange_vault_with_time_window(store, time_window)?
                                .output(());
                            prepare.merge(request)
                        } else {
                            request
                        };
                        rpc
                    }
                    (None, Some(exchange)) => {
                        client
                            .complete_gt_exchange(store, exchange, None, None, None)
                            .await?
                    }
                    (None, None) => {
                        let owner = owner.unwrap_or(client.payer());
                        let exchanges = client.gt_exchanges(store, &owner).await?;

                        let output = ctx.config().output();
                        let options = DisplayOptions::table_projection([
                            ("address", "Pubkey"),
                            ("vault", "Vault"),
                            ("amount", "Amount"),
                        ]);

                        let items = exchanges.into_iter().map(|(address, exchange)| {
                            let amount =
                                unsigned_amount_to_decimal(exchange.amount, decimals).normalize();
                            serde_json::json!({
                                "address": StringPubkey(address),
                                "vault": StringPubkey(exchange.vault),
                                "amount": amount,
                            })
                        });

                        println!("{}", output.display_many(items, options)?);
                        return Ok(());
                    }
                    _ => {
                        unreachable!()
                    }
                }
            }
        };

        let bundle = txn.into_bundle_with_options(options)?;

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
