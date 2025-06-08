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

#[cfg(feature = "execute")]
// use crate::commands::exchange::executor;
use {
    crate::commands::exchange::executor,
    gmsol_sdk::{
        client::pyth::{pubkey_to_identifier, pull_oracle::hermes::Identifier, Hermes},
        core::oracle::{pyth_price_with_confidence_to_price, PriceProviderKind},
        core::token_config::TokenConfig,
        programs::gmsol_store::accounts::Market,
        programs::gmsol_treasury::accounts::{Config, TreasuryVaultConfig},
        utils::zero_copy::ZeroCopy,
    },
    rust_decimal::Decimal,
    std::{collections::HashMap, num::NonZeroU8, sync::Arc},
};

/// Structure to store token value information
#[cfg(feature = "execute")]
#[derive(Debug)]
struct TokenValueInfo {
    market: Arc<Market>,
    is_long: bool,
    amount: u64,
    unit_price: u128,
    value: Value,
}

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
        #[arg(long)]
        market_token: Option<Pubkey>,
        #[arg(long)]
        side: Option<Side>,
        #[arg(long)]
        deposit: bool,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
        #[arg(long, short, default_value_t = Amount::ZERO)]
        min_amount: Amount,
        #[cfg(feature = "execute")]
        #[arg(long, default_value_t = Amount(Decimal::from(1000)))]
        min_value_per_batch: Amount,
        #[cfg(feature = "execute")]
        #[arg(long, default_value_t = NonZeroU8::new(3).unwrap())]
        batch: NonZeroU8,
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

        let bundle = match &self.command {
            Command::InitConfig => {
                let (rpc, config) = client.initialize_config(store).swap_output(());
                println!("{config}");
                rpc.into_bundle_with_options(options)?
            }
            Command::InitTreasury { index } => {
                let (rpc, address) = client
                    .initialize_treasury_vault_config(store, *index)
                    .swap_output(());
                println!("{address}");
                rpc.into_bundle_with_options(options)?
            }
            Command::TransferReceiver { new_receiver } => client
                .transfer_receiver(store, new_receiver)
                .into_bundle_with_options(options)?,
            Command::SetTreasury {
                treasury_vault_config,
            } => client
                .set_treasury_vault_config(store, treasury_vault_config)
                .into_bundle_with_options(options)?,
            Command::SetGtFactor { factor } => client
                .set_gt_factor(store, factor.to_u128()?)?
                .into_bundle_with_options(options)?,
            Command::SetBuybackFactor { factor } => client
                .set_buyback_factor(store, factor.to_u128()?)?
                .into_bundle_with_options(options)?,
            Command::InsertToken { token } => client
                .insert_token_to_treasury(store, None, token)
                .await?
                .into_bundle_with_options(options)?,
            Command::RemoveToken { token } => client
                .remove_token_from_treasury(store, None, token)
                .await?
                .into_bundle_with_options(options)?,
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
                    .into_bundle_with_options(options)?
            }
            Command::SetReferralReward { factors } => {
                if factors.is_empty() {
                    return Err(eyre::eyre!("factors must be provided"));
                }
                let factors = factors
                    .iter()
                    .map(|f| f.to_u128().map_err(eyre::Report::from))
                    .collect::<eyre::Result<Vec<_>>>()?;
                client
                    .set_referral_reward(store, factors)
                    .into_bundle_with_options(options)?
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
                #[cfg(feature = "execute")]
                min_value_per_batch,
                #[cfg(feature = "execute")]
                batch,
            } => {
                if let Some(market_token) = market_token {
                    // Single market mode
                    let side = side.ok_or_eyre("side is required for single market mode")?;
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
                        let mut bundle = client.bundle_with_options(options.clone());
                        bundle.push(claim.merge(deposit))?;
                        bundle
                    } else {
                        let mut bundle = client.bundle_with_options(options.clone());
                        bundle.push(claim)?;
                        bundle
                    }
                } else {
                    #[cfg(feature = "execute")]
                    {
                        // Batch processing mode with Pyth support
                        let markets = client.markets(store).await?;
                        let mut claimable_fees = Vec::new();

                        // Step 1: Collect all market claimable fees
                        for (_, market) in markets {
                            let market_model = MarketModel::from_parts(market.clone(), 1);
                            if let Ok(fee_pool) = market_model.claimable_fee_pool() {
                                if side.is_none() || side.unwrap().is_long() {
                                    if let Ok(amount) = fee_pool.amount(true) {
                                        if amount > 0 {
                                            claimable_fees.push((market.clone(), true, amount));
                                        }
                                    }
                                }
                                // Skip short token processing for single token pool when no side is specified
                                if (side.is_none() || !side.unwrap().is_long())
                                    && !(side.is_none()
                                        && market.meta.long_token_mint
                                            == market.meta.short_token_mint)
                                {
                                    if let Ok(amount) = fee_pool.amount(false) {
                                        if amount > 0 {
                                            claimable_fees.push((market.clone(), false, amount));
                                        }
                                    }
                                }
                            }
                        }

                        // Step 2: Fetch all prices from Pyth using Hermes
                        let hermes = Hermes::default();

                        // Helper function to get token config
                        let get_token_config = |token_mint: &Pubkey| -> eyre::Result<&TokenConfig> {
                            token_map
                                .get(token_mint)
                                .ok_or_eyre("token config not found")
                        };

                        let mut feed_to_token = HashMap::new();
                        for (market, is_long, _) in &claimable_fees {
                            let token_mint = if *is_long {
                                &market.meta.long_token_mint
                            } else {
                                &market.meta.short_token_mint
                            };

                            let token_config = get_token_config(token_mint)?;

                            let feed = token_config
                                .get_feed(&PriceProviderKind::Pyth)
                                .map_err(|_| eyre::eyre!("no Pyth feed found for token"))?;

                            feed_to_token
                                .insert(pubkey_to_identifier(&feed), (*token_mint, *token_config));
                        }

                        let feed_ids: Vec<_> = feed_to_token.keys().cloned().collect();
                        let update = hermes.latest_price_updates(&feed_ids, None).await?;

                        // Process price updates and create price mapping
                        let mut price_map = HashMap::new();
                        for price in update.parsed() {
                            let feed_id = Identifier::from_hex(price.id())
                                .map_err(|e| eyre::eyre!("Failed to parse feed id: {}", e))?;
                            if let Some((token_mint, token_config)) = feed_to_token.get(&feed_id) {
                                let price = pyth_price_with_confidence_to_price(
                                    price.price().price(),
                                    price.price().conf(),
                                    price.price().expo(),
                                    token_config,
                                )?;
                                price_map.insert(*token_mint, price);
                            }
                        }

                        // New: Calculate and store token values
                        let mut token_value_infos = Vec::new();
                        for (market, is_long, amount) in &claimable_fees {
                            let token_mint = if *is_long {
                                &market.meta.long_token_mint
                            } else {
                                &market.meta.short_token_mint
                            };

                            let price = price_map
                                .get(token_mint)
                                .ok_or_eyre("price not found in price map")?;
                            let unit_price = price.min.to_unit_price();
                            // amount is already in unit token, unit_price is in 10^-20 USD / unit token
                            let value = Value::from_u128(*amount * unit_price);
                            // println!("DEBUG: Creating TokenValueInfo - token: {}, amount: {}, unit_price: {}, value: {}",
                            //     token_mint, amount, unit_price, value);
                            token_value_infos.push(TokenValueInfo {
                                market: market.clone(),
                                is_long: *is_long,
                                amount: *amount as u64,
                                unit_price,
                                value,
                            });
                        }

                        // New: Sort token values
                        token_value_infos.sort_by(|a, b| b.value.cmp(&a.value));

                        // Step 3: Build transaction batches based on min_value_per_batch and batch size
                        // Use chunks to create batches
                        let mut batches: Vec<Vec<(Arc<Market>, bool, Amount)>> = Vec::new();
                        let batch_size = batch.get() as usize;

                        for chunk in token_value_infos.chunks(batch_size) {
                            let mut chunk_value = Decimal::ZERO;
                            // let mut valid_chunk = true;

                            // Calculate total value for this chunk
                            for info in chunk {
                                chunk_value += info.value.0;
                            }

                            // Only add chunks that meet the minimum value requirement
                            if chunk_value >= min_value_per_batch.0 {
                                // Convert the chunk to the correct type
                                let converted_chunk: Vec<(Arc<Market>, bool, Amount)> = chunk
                                    .iter()
                                    .map(|info| {
                                        let token_mint = if info.is_long {
                                            &info.market.meta.long_token_mint
                                        } else {
                                            &info.market.meta.short_token_mint
                                        };
                                        let token_config = get_token_config(token_mint).unwrap();
                                        let amount = Amount::from_u64(
                                            info.amount,
                                            token_config.token_decimals,
                                        );
                                        (info.market.clone(), info.is_long, amount)
                                    })
                                    .collect();
                                batches.push(converted_chunk);
                            } else {
                                // Since claims are sorted by value, if this chunk doesn't meet the threshold,
                                // subsequent chunks won't either
                                break;
                            }
                        }

                        // Step 4: Build bundle with claim transactions and optional deposit instructions
                        let mut bundle = client.bundle_with_options(options.clone());
                        let mut claimed_tokens = HashMap::new();

                        for batch in batches {
                            let mut batch_builder = client.store_transaction();

                            for (market, is_long, amount) in batch {
                                let token_mint = if is_long {
                                    &market.meta.long_token_mint
                                } else {
                                    &market.meta.short_token_mint
                                };

                                let token_config = get_token_config(token_mint)?;

                                let claim = client.claim_fees_to_receiver_vault(
                                    store,
                                    &market.meta.market_token_mint,
                                    token_mint,
                                    amount.to_u64(token_config.token_decimals)?,
                                );
                                batch_builder = batch_builder.merge(claim);

                                *claimed_tokens.entry(*token_mint).or_insert(0) +=
                                    amount.to_u64(token_config.token_decimals)?;
                            }

                            bundle.push(batch_builder)?;
                        }

                        // Add deposit instructions if --deposit is specified
                        if *deposit {
                            // println!("Skipping all token deposits temporarily");
                            // return Ok(());

                            let store_account = client.store(store).await?;
                            let time_window = store_account.gt.exchange_time_window;

                            // Get treasury vault config
                            let config = client.find_treasury_config_address(store);
                            // println!("Treasury global config address: {}", config);

                            let config_account = client
                                .account::<ZeroCopy<Config>>(&config)
                                .await?
                                .ok_or_eyre("treasury config not found")?;

                            let treasury_vault_config = config_account.0.treasury_vault_config;
                            // println!("Treasury vault config address: {}", treasury_vault_config);

                            let treasury_vault_config = client
                                .account::<ZeroCopy<TreasuryVaultConfig>>(&treasury_vault_config)
                                .await?
                                .ok_or_eyre("treasury vault config not found")?;

                            for token_mint in claimed_tokens.keys() {
                                // Skip if token doesn't exist or deposit is not allowed
                                if !treasury_vault_config
                                    .0
                                    .tokens
                                    .get(token_mint)
                                    .map(|config| config.flags.get_flag(TokenFlag::AllowDeposit))
                                    .unwrap_or(false)
                                {
                                    println!(
                                        "Skipping deposit for token {} as it is not allowed",
                                        token_mint
                                    );
                                    continue;
                                }

                                let (deposit, _) = client
                                    .deposit_to_treasury_valut(
                                        store,
                                        None,
                                        token_mint,
                                        token_program_id.as_ref(),
                                        time_window,
                                    )
                                    .await?
                                    .swap_output(());
                                bundle.push(deposit)?;
                            }
                        }

                        // Step 5: Display claimed values and token amounts in human-readable format
                        let mut sorted_tokens: Vec<_> = claimed_tokens.into_iter().collect();
                        sorted_tokens.sort_by_key(|(mint, _)| *mint);

                        let mut total_value = Value::ZERO;
                        for (token_mint, _) in &sorted_tokens {
                            let token_config = get_token_config(token_mint)?;

                            let mut total_amount = 0;
                            let mut token_total_value = Value::ZERO;
                            let mut unit_price = 0;

                            for info in token_value_infos.iter() {
                                let info_token_mint = if info.is_long {
                                    &info.market.meta.long_token_mint
                                } else {
                                    &info.market.meta.short_token_mint
                                };
                                if info_token_mint == token_mint {
                                    total_amount += info.amount;
                                    token_total_value = Value::from_u128(
                                        token_total_value.to_u128().unwrap_or(0)
                                            + info.value.to_u128().unwrap_or(0),
                                    );
                                    unit_price = info.unit_price;
                                }
                            }

                            total_value = Value::from_u128(
                                total_value.to_u128().unwrap_or(0)
                                    + token_total_value.to_u128().unwrap_or(0),
                            );

                            println!(
                                "Token {}: {} (Price: ${}, Value: ${})",
                                token_mint,
                                Amount::from_u64(total_amount, token_config.token_decimals),
                                Value::from_u128(
                                    unit_price * 10u128.pow(token_config.token_decimals as u32)
                                ),
                                token_total_value
                            );
                        }

                        println!("Total value claimed: ${}", total_value);

                        bundle
                    }
                    #[cfg(not(feature = "execute"))]
                    {
                        return Err(eyre::eyre!(
                            "Batch processing mode requires the 'execute' feature to be enabled"
                        ));
                    }
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

                rpc.into_bundle_with_options(options)?
            }
            Command::PrepareGtBank { gt_exchange_vault } => {
                let gt_exchange_vault = gt_exchange_vault.get(store, client).await?;
                let (txn, gt_bank) = client
                    .prepare_gt_bank(store, None, &gt_exchange_vault)
                    .await?
                    .swap_output(());

                tracing::info!("Preparing GT bank: {gt_bank}");
                println!("{gt_bank}");

                txn.into_bundle_with_options(options)?
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
                    .into_bundle_with_options(options)?
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
                    fund.merge(rpc).into_bundle_with_options(options)?
                } else {
                    rpc.into_bundle_with_options(options)?
                }
            }
            Command::CancelSwap { order } => client
                .cancel_treasury_swap(store, order, None)
                .await?
                .into_bundle_with_options(options)?,
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
                    .into_bundle_with_options(options)?
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

                bundle
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
    anchor_spl::token::ID.into()
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
