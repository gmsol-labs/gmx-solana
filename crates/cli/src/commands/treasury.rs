use std::path::PathBuf;

use crate::commands::utils::token_amount;
use anchor_spl::{
    associated_token::{
        get_associated_token_address, get_associated_token_address_with_program_id,
    },
    token_interface::TokenAccount,
};
use eyre::OptionExt;
use gmsol_sdk::{
    client::ops::treasury::CreateTreasurySwapOptions,
    core::token_config::{TokenFlag, TokenMapAccess},
    model::{BalanceExt, BaseMarket, MarketModel},
    ops::{system::SystemProgramOps, token_account::TokenAccountOps, treasury::TreasuryOps},
    programs::anchor_lang::prelude::Pubkey,
    serde::StringPubkey,
    solana_utils::bundle_builder::BundleOptions,
    utils::{Amount, Lamport, Value},
};

/// Read and parse a TOML file into a type
fn toml_from_file<T>(path: &impl AsRef<std::path::Path>) -> eyre::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    use std::io::Read;
    let mut buffer = String::new();
    std::fs::File::open(path)?.read_to_string(&mut buffer)?;
    toml::from_str(&buffer).map_err(|e| eyre::eyre!("Failed to parse TOML: {}", e))
}

#[cfg(feature = "execute")]
use crate::commands::exchange::executor;

/// Treasury management commands.
#[derive(Debug, clap::Args)]
pub struct Treasury {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Initialize Global Config.
    InitConfig,
    /// Initialize Treasury.
    InitTreasury { index: u16 },
    /// Transfer Receiver.
    TransferReceiver {
        #[arg(long)]
        new_receiver: Pubkey,
    },
    /// Set treasury.
    SetTreasury { treasury_vault_config: Pubkey },
    /// Set GT factor.
    SetGtFactor { factor: Value },
    /// Set Buyback factor.
    SetBuybackFactor { factor: Value },
    /// Insert token to the treasury.
    InsertToken { token: Pubkey },
    /// Remove token from the treasury.
    RemoveToken { token: Pubkey },
    /// Toggle token flag.
    ToggleTokenFlag {
        token: Pubkey,
        #[arg(requires = "toggle")]
        flag: TokenFlag,
        /// Enable the given flag.
        #[arg(long, group = "toggle")]
        enable: bool,
        /// Disable the given flag.
        #[arg(long, group = "toggle")]
        disable: bool,
    },
    /// Set referral reward factors.
    SetReferralReward { factors: Vec<Value> },
    /// Claim fees.
    ClaimFees {
        market_token: Pubkey,
        #[arg(long)]
        side: Side,
        #[arg(long)]
        deposit: bool,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
        #[arg(long, short, default_value_t = Amount::ZERO)]
        min_amount: Amount,
    },
    /// Deposit into treasury vault.
    DepositToTreasury {
        token_mint: Pubkey,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
    },
    /// Prepare GT bank.
    PrepareGtBank {
        #[clap(flatten)]
        gt_exchange_vault: SelectGtExchangeVault,
    },
    /// Confirm GT buyback.
    #[cfg(feature = "execute")]
    ConfirmGtBuyback {
        #[clap(flatten)]
        gt_exchange_vault: SelectGtExchangeVault,
        #[arg(long)]
        oracle: Pubkey,
        #[command(flatten)]
        args: executor::ExecutorArgs,
    },
    /// Sync GT bank.
    SyncGtBank {
        token_mint: Pubkey,
        #[clap(flatten)]
        gt_exchange_vault: SelectGtExchangeVault,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
    },
    /// Create Swap.
    CreateSwap {
        market_token: Pubkey,
        #[arg(long, short = 'i')]
        swap_in: Pubkey,
        #[arg(long, short = 'o')]
        swap_out: Pubkey,
        /// Swap in amount.
        #[arg(long, short)]
        amount: Option<Amount>,
        #[arg(long)]
        min_output_amount: Option<Amount>,
        /// Extra swap paths.
        #[arg(long, short = 's', action = clap::ArgAction::Append)]
        extra_swap_path: Vec<Pubkey>,
        /// Fund the swap owner.
        #[arg(long, value_name = "LAMPORTS")]
        fund: Option<Lamport>,
    },
    /// Cancel Swap.
    CancelSwap { order: Pubkey },
    /// Get Receiver Address.
    Receiver,
    /// Withdraw from the treasury vault.
    Withdraw {
        token: Pubkey,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
        #[arg(long, short)]
        amount: Amount,
        #[arg(long)]
        target: Option<Pubkey>,
    },
    /// Batch withdraw.
    BatchWithdraw {
        file: PathBuf,
        #[arg(long)]
        force_one_tx: bool,
    },
}

impl super::Command for Treasury {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();
        let token_map = client.authorized_token_map(store).await?;

        let txn = match &self.command {
            Command::InitConfig => {
                let (rpc, config) = client.initialize_config(store).swap_output(());
                println!("{config}");
                rpc
            }
            Command::InitTreasury { index } => {
                let (rpc, address) = client
                    .initialize_treasury_vault_config(store, *index)
                    .swap_output(());
                println!("{address}");
                rpc
            }
            Command::TransferReceiver { new_receiver } => {
                client.transfer_receiver(store, new_receiver)
            }
            Command::SetTreasury {
                treasury_vault_config,
            } => client.set_treasury_vault_config(store, treasury_vault_config),
            Command::SetGtFactor { factor } => client.set_gt_factor(store, factor.to_u128()?)?,
            Command::SetBuybackFactor { factor } => {
                client.set_buyback_factor(store, factor.to_u128()?)?
            }
            Command::InsertToken { token } => {
                client.insert_token_to_treasury(store, None, token).await?
            }
            Command::RemoveToken { token } => {
                client
                    .remove_token_from_treasury(store, None, token)
                    .await?
            }
            Command::ToggleTokenFlag {
                token,
                flag,
                enable,
                disable,
            } => {
                assert!(*enable != *disable);
                let value = *enable;
                client
                    .toggle_token_flag(store, None, token, *flag, value)
                    .await?
            }
            Command::SetReferralReward { factors } => {
                if factors.is_empty() {
                    return Err(eyre::eyre!("factors must be provided"));
                }
                let factors = factors
                    .iter()
                    .map(|f| f.to_u128().map_err(eyre::Report::from))
                    .collect::<eyre::Result<Vec<_>>>()?;
                client.set_referral_reward(store, factors)
            }
            Command::Receiver => {
                let config = client.find_treasury_config_address(store);
                let receiver = client.find_treasury_receiver_address(&config);
                println!("{receiver}");
                return Ok(());
            }
            Command::ClaimFees {
                market_token,
                side,
                deposit,
                token_program_id,
                min_amount,
            } => {
                let market = client.find_market_address(store, market_token);
                let market = client.market(&market).await?;
                let market_model = MarketModel::from_parts(market.clone(), 1);
                let amount = market_model.claimable_fee_pool()?.amount(side.is_long())?;
                if amount == 0 {
                    return Err(eyre::eyre!("no claimable fees for this side"));
                }
                let token_mint = if side.is_long() {
                    &market.meta.long_token_mint
                } else {
                    &market.meta.short_token_mint
                };
                let min_amount = token_amount(
                    min_amount,
                    Some(token_mint),
                    &token_map,
                    &market_model,
                    side.is_long(),
                )?;
                let claim = client.claim_fees_to_receiver_vault(
                    store,
                    market_token,
                    token_mint,
                    min_amount,
                );

                if *deposit {
                    let store_account = client.store(store).await?;
                    let time_window = store_account.gt.exchange_time_window;
                    let (deposit, gt_exchange_vault) = client
                        .deposit_to_treasury_valut(
                            store,
                            None,
                            token_mint,
                            token_program_id.as_ref(),
                            time_window,
                        )
                        .await?
                        .swap_output(());
                    println!("{gt_exchange_vault}");
                    claim.merge(deposit)
                } else {
                    claim
                }
            }
            Command::DepositToTreasury {
                token_mint,
                token_program_id,
            } => {
                let store_account = client.store(store).await?;
                let time_window = store_account.gt.exchange_time_window;

                let (rpc, gt_exchange_vault) = client
                    .deposit_to_treasury_valut(
                        store,
                        None,
                        token_mint,
                        token_program_id.as_ref(),
                        time_window,
                    )
                    .await?
                    .swap_output(());
                println!("{gt_exchange_vault}");

                rpc
            }
            Command::PrepareGtBank { gt_exchange_vault } => {
                let gt_exchange_vault = gt_exchange_vault.get(store, client).await?;
                let (txn, gt_bank) = client
                    .prepare_gt_bank(store, None, &gt_exchange_vault)
                    .await?
                    .swap_output(());

                tracing::info!("Preparing GT bank: {gt_bank}");
                println!("{gt_bank}");

                txn
            }
            Command::SyncGtBank {
                gt_exchange_vault,
                token_mint,
                token_program_id,
            } => {
                let gt_exchange_vault = gt_exchange_vault.get(store, client).await?;
                client
                    .sync_gt_bank(
                        store,
                        None,
                        &gt_exchange_vault,
                        token_mint,
                        token_program_id.as_ref(),
                    )
                    .await?
            }
            Command::CreateSwap {
                market_token,
                swap_in,
                swap_out,
                amount,
                min_output_amount,
                extra_swap_path,
                fund,
            } => {
                let config = client.find_treasury_config_address(store);
                let receiver = client.find_treasury_receiver_address(&config);
                let market = client.find_market_address(store, market_token);
                let market = client.market(&market).await?;
                // let meta = &market.meta;
                let amount = match amount {
                    Some(amount) => token_amount(amount, Some(swap_in), &token_map, &market, true)?,
                    None => {
                        let vault = get_associated_token_address(&receiver, swap_in);
                        let account = client
                            .account::<TokenAccount>(&vault)
                            .await?
                            .ok_or_eyre("vault account is not initialized")?;
                        account.amount
                    }
                };
                let min_output_amount = min_output_amount
                    .map(|amount| token_amount(&amount, Some(swap_out), &token_map, &market, false))
                    .transpose()?;
                let (rpc, order) = client
                    .create_treasury_swap(
                        store,
                        market_token,
                        swap_in,
                        swap_out,
                        amount,
                        CreateTreasurySwapOptions {
                            swap_path: extra_swap_path.clone(),
                            min_swap_out_amount: min_output_amount,
                            ..Default::default()
                        },
                    )
                    .await?
                    .swap_output(());
                println!("{order}");
                if let Some(lamports) = fund {
                    let swap_owner = client.find_treasury_receiver_address(&config);
                    let fund = client.transfer(&swap_owner, lamports.to_u64()?)?;
                    fund.merge(rpc)
                } else {
                    rpc
                }
            }
            Command::CancelSwap { order } => {
                client.cancel_treasury_swap(store, order, None).await?
            }
            Command::Withdraw {
                token,
                token_program_id,
                amount,
                target,
            } => {
                let target = target.unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(
                        &client.payer(),
                        token,
                        &token_program_id.unwrap_or(anchor_spl::token::ID),
                    )
                });
                let decimals = token_map
                    .get(token)
                    .ok_or_eyre("token config not found")?
                    .token_decimals;
                client
                    .withdraw_from_treasury_vault(
                        store,
                        None,
                        token,
                        token_program_id.as_ref(),
                        amount.to_u64(decimals)?,
                        decimals,
                        &target,
                    )
                    .await?
            }
            Command::BatchWithdraw { file, force_one_tx } => {
                let batch = toml_from_file::<BatchWithdraw>(file)?;

                if batch.withdraw.is_empty() {
                    return Ok(());
                }

                let mut bundle = client.bundle_with_options(BundleOptions {
                    force_one_transaction: *force_one_tx,
                    ..Default::default()
                });

                for withdraw in batch.withdraw {
                    let target = get_associated_token_address_with_program_id(
                        &withdraw.target,
                        &withdraw.token,
                        &withdraw.token_program_id,
                    );
                    let decimals = if let Some(decimals) = withdraw.token_decimals {
                        decimals
                    } else {
                        client
                            .account::<anchor_spl::token_interface::Mint>(&withdraw.token)
                            .await?
                            .ok_or_eyre("token mint not found")?
                            .decimals
                    };
                    let amount = withdraw.amount.to_u64(decimals)?;
                    let prepare = client.prepare_associated_token_account(
                        &withdraw.token,
                        &withdraw.token_program_id,
                        Some(&withdraw.target),
                    );
                    let txn = client
                        .withdraw_from_treasury_vault(
                            store,
                            None,
                            &withdraw.token,
                            Some(&withdraw.token_program_id),
                            amount,
                            decimals,
                            &target,
                        )
                        .await?;
                    bundle.push(prepare.merge(txn))?;
                }

                for sync in batch.sync {
                    bundle.push(
                        client
                            .sync_gt_bank(
                                store,
                                None,
                                &sync.gt_exchange_vault,
                                &sync.token,
                                Some(&sync.token_program_id),
                            )
                            .await?,
                    )?;
                }

                return Ok(client.send_or_serialize(bundle).await?);
            }
            #[cfg(feature = "execute")]
            Command::ConfirmGtBuyback {
                gt_exchange_vault,
                oracle,
                args,
            } => {
                let gt_exchange_vault = gt_exchange_vault.get(store, client).await?;
                let builder = client.confirm_gt_buyback(store, &gt_exchange_vault, oracle);
                let executor = args.build(client).await?;
                executor.execute(builder, options).await?;
                return Ok(());
            }
        };

        let bundle = txn.into_bundle_with_options(options)?;

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}

/// Side.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum Side {
    /// Long.
    Long,
    /// Short.
    Short,
}

impl Side {
    /// Is long side.
    pub fn is_long(&self) -> bool {
        matches!(self, Self::Long)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Withdraw {
    token: StringPubkey,
    #[serde(default = "default_token_program_id")]
    token_program_id: StringPubkey,
    #[serde(default)]
    token_decimals: Option<u8>,
    amount: Amount,
    target: StringPubkey,
}

fn default_token_program_id() -> StringPubkey {
    spl_token::ID.into()
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Sync {
    token: StringPubkey,
    #[serde(default = "default_token_program_id")]
    token_program_id: StringPubkey,
    gt_exchange_vault: StringPubkey,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct BatchWithdraw {
    #[serde(default)]
    withdraw: Vec<Withdraw>,
    #[serde(default)]
    sync: Vec<Sync>,
}

/// Select GT Exchange Vault by date or direct address
#[derive(clap::Args, Clone, Debug)]
pub struct SelectGtExchangeVault {
    /// Direct GT Exchange Vault address
    gt_exchange_vault: Option<Pubkey>,
    /// Select by date
    #[clap(flatten)]
    date: SelectGtExchangeVaultByDate,
}

/// Select GT Exchange Vault by date
#[derive(clap::Args, Clone, Debug)]
pub struct SelectGtExchangeVaultByDate {
    /// Date to select vault
    #[arg(long, short)]
    date: Option<humantime::Timestamp>,
}

impl SelectGtExchangeVault {
    /// Get GT Exchange Vault address
    pub async fn get(
        &self,
        store: &Pubkey,
        client: &crate::commands::CommandClient,
    ) -> eyre::Result<Pubkey> {
        if let Some(address) = self.gt_exchange_vault {
            Ok(address)
        } else {
            self.date.get(store, client).await
        }
    }
}

impl SelectGtExchangeVaultByDate {
    /// Get GT Exchange Vault address by date
    pub async fn get(
        &self,
        store: &Pubkey,
        client: &crate::commands::CommandClient,
    ) -> eyre::Result<Pubkey> {
        use std::time::SystemTime;

        let store_account = client.store(store).await?;
        let time_window = store_account.gt.exchange_time_window;
        let date = self
            .date
            .as_ref()
            .cloned()
            .unwrap_or_else(|| humantime::Timestamp::from(SystemTime::now()));
        let ts = date
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| eyre::eyre!("Failed to get timestamp: {}", e))?
            .as_secs();
        let index = ts / time_window as u64;
        Ok(client.find_gt_exchange_vault_address(store, index as i64, time_window))
    }
}
