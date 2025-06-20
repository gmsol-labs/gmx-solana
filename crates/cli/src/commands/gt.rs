use crate::config::DisplayOptions;
use eyre::OptionExt;
use gmsol_sdk::{
    core::{gt::GtBankFlags, pubkey::optional_address, token_config::TokenMapAccess},
    ops::{
        gt::{current_time_window_index, GtOps},
        treasury::TreasuryOps,
    },
    programs::{
        anchor_lang::prelude::Pubkey,
        gmsol_store::accounts::GtExchangeVault,
        gmsol_treasury::accounts::{Config, GtBank},
    },
    serde::StringPubkey,
    solana_utils::solana_sdk::signer::Signer,
    utils::{unsigned_amount_to_decimal, zero_copy::ZeroCopy, Amount},
};
use std::{num::NonZeroU32, ops::Deref};

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
    /// Get GT exchange vault.
    GtExchangeVault { address: Option<Pubkey> },
    /// GT Bank.
    GtBank {
        address: Option<Pubkey>,
        #[arg(long, value_name = "GT_EXCHANGE_VAULT")]
        vault: Option<Pubkey>,
        #[clap(flatten)]
        date: SelectGtExchangeVaultByDate,
        #[arg(long)]
        debug: bool,
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
            Command::GtExchangeVault { address } => {
                let address = match address {
                    Some(address) => *address,
                    None => {
                        let time_window = client.store(store).await?.gt.exchange_time_window;
                        let time_window_index = current_time_window_index(time_window)?;
                        client.find_gt_exchange_vault_address(store, time_window_index, time_window)
                    }
                };
                let vault = client
                    .account::<ZeroCopy<GtExchangeVault>>(&address)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?
                    .0;
                println!("{vault:#?}");
                return Ok(());
            }
            Command::GtBank {
                address,
                vault,
                date,
                debug,
            } => {
                let address = if let Some(address) = address {
                    *address
                } else {
                    let vault = match vault {
                        Some(vault) => *vault,
                        None => date.get(store, client).await?,
                    };
                    let config = client.find_treasury_config_address(store);
                    let config = client
                        .account::<ZeroCopy<Config>>(&config)
                        .await?
                        .ok_or(gmsol_sdk::Error::NotFound)?
                        .0;
                    let treasury_vault_config = optional_address(&config.treasury_vault_config)
                        .ok_or_else(|| {
                            gmsol_sdk::Error::custom("treasury vault config is not set")
                        })?;
                    client.find_gt_bank_address(treasury_vault_config, &vault)
                };

                let bank = client
                    .account::<ZeroCopy<GtBank>>(&address)
                    .await?
                    .ok_or(gmsol_sdk::Error::NotFound)?
                    .0;
                if *debug {
                    println!("{bank:#?}");
                } else {
                    println!("Address: {address}");
                    println!("Treasury Vault Config: {}", bank.treasury_vault_config);
                    println!("GT Exchange Vault: {}", bank.gt_exchange_vault);
                    let mut status = String::default();
                    let is_confirmed = bank.flags.get_flag(GtBankFlags::Confirmed);
                    if is_confirmed {
                        status.push_str("confirmed");
                    } else {
                        status.push_str("not confirmed");
                    }
                    if bank.flags.get_flag(GtBankFlags::SyncedAfterConfirmation) {
                        status.push_str("synced");
                    }
                    println!("Status: {status}");

                    let store = client.store(store).await?;
                    let gt_decimals = store.gt.decimals;

                    if is_confirmed {
                        println!(
                            "Remaining GT: {}",
                            Amount::from_u64(bank.remaining_confirmed_gt_amount, gt_decimals)
                        );
                    }

                    let token_map_address =
                        optional_address(&store.token_map).ok_or_eyre("no authorized token map")?;
                    let token_map = client.token_map(token_map_address).await?;
                    println!("[Balances]");
                    for (token, balance) in bank.balances.entries() {
                        let token = Pubkey::new_from_array(*token);
                        let token_decimals = token_map
                            .get(&token)
                            .ok_or(gmsol_sdk::Error::NotFound)?
                            .token_decimals;
                        let recevier_vault_out =
                            Amount::from_u64(balance.receiver_vault_out, token_decimals);
                        let balance = Amount::from_u64(balance.amount, token_decimals);

                        println!(
                            "{token}: balance = {}, receiver_vault_out = {}",
                            balance, recevier_vault_out
                        );
                    }
                }

                return Ok(());
            }
        };

        let bundle = txn.into_bundle_with_options(options)?;

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

#[derive(clap::Args, Clone, Debug)]
pub(crate) struct SelectGtExchangeVaultByDate {
    #[arg(long, short)]
    date: Option<humantime::Timestamp>,
}

impl SelectGtExchangeVaultByDate {
    pub(crate) async fn get<C: Deref<Target = impl Signer> + Clone>(
        &self,
        store: &Pubkey,
        client: &gmsol_sdk::Client<C>,
    ) -> gmsol_sdk::Result<Pubkey> {
        use std::time::SystemTime;

        let time_window = client.store(store).await?.gt.exchange_time_window;
        let date = self
            .date
            .as_ref()
            .cloned()
            .unwrap_or_else(|| humantime::Timestamp::from(SystemTime::now()));
        let ts = date
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(gmsol_sdk::Error::custom)?
            .as_secs();
        let index = ts / time_window as u64;
        Ok(client.find_gt_exchange_vault_address(store, index as i64, time_window))
    }
}
