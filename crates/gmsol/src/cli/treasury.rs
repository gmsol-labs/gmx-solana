use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_spl::{
    associated_token::{
        get_associated_token_address, get_associated_token_address_with_program_id,
    },
    token_interface::TokenAccount,
};
use gmsol::{
    chainlink::{self, pull_oracle::ChainlinkPullOracleFactory},
    client::SystemProgramOps,
    pyth::{pull_oracle::PythPullOracleWithHermes, Hermes, PythPullOracle},
    treasury::{CreateTreasurySwapOptions, TreasuryOps},
    utils::{
        builder::{MakeBundleBuilder, WithPullOracle},
        instruction::InstructionSerialization,
    },
};
use gmsol_model::{BalanceExt, BaseMarket};
use gmsol_solana_utils::bundle_builder::BundleOptions;
use gmsol_treasury::states::treasury::TokenFlag;

use crate::{
    utils::{SelectGtExchangeVault, Side},
    GMSOLClient, InstructionBufferCtx,
};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
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
    SetGtFactor { factor: u128 },
    /// Set Buyback factor.
    SetBuybackFactor { factor: u128 },
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
    SetReferralReward { factors: Vec<u128> },
    /// Claim fees.
    ClaimFees {
        market_token: Pubkey,
        #[arg(long)]
        side: Side,
        #[arg(long)]
        deposit: bool,
        #[arg(long)]
        token_program_id: Option<Pubkey>,
        #[arg(long, short, default_value_t = 1)]
        min_amount: u64,
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
    ConfirmGtBuyback {
        #[clap(flatten)]
        gt_exchange_vault: SelectGtExchangeVault,
        #[arg(long)]
        oracle: Pubkey,
        #[arg(long, short = 't')]
        oracle_testnet: bool,
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
        amount: Option<u64>,
        #[arg(long)]
        min_output_amount: Option<u64>,
        /// Extra swap paths.
        #[arg(long, short = 's', action = clap::ArgAction::Append)]
        extra_swap_path: Vec<Pubkey>,
        /// Fund the swap owner.
        #[arg(long, value_name = "LAMPORTS")]
        fund: Option<u64>,
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
        amount: u64,
        #[arg(long, short)]
        decimals: u8,
        #[arg(long)]
        target: Option<Pubkey>,
    },
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        let req = match &self.command {
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
            Command::SetGtFactor { factor } => client.set_gt_factor(store, *factor)?,
            Command::SetBuybackFactor { factor } => client.set_buyback_factor(store, *factor)?,
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
                    return Err(gmsol::Error::invalid_argument("factors must be provided"));
                }
                client.set_referral_reward(store, factors.clone())
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
                let amount = market.claimable_fee_pool()?.amount(side.is_long())?;
                if amount == 0 {
                    return Err(gmsol::Error::invalid_argument(
                        "no claimable fees for this side",
                    ));
                }
                let token_mint = market.meta().pnl_token(side.is_long());
                let claim = client.claim_fees_to_receiver_vault(
                    store,
                    market_token,
                    &token_mint,
                    *min_amount,
                );

                if *deposit {
                    let store_account = client.store(store).await?;
                    let time_window = store_account.gt().exchange_time_window();
                    let (deposit, gt_exchange_vault) = client
                        .deposit_to_treasury_valut(
                            store,
                            None,
                            &token_mint,
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
                let time_window = store_account.gt().exchange_time_window();

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
            Command::ConfirmGtBuyback {
                gt_exchange_vault,
                oracle,
                oracle_testnet,
            } => {
                let gt_exchange_vault = gt_exchange_vault.get(store, client).await?;
                let builder = client.confirm_gt_buyback(store, &gt_exchange_vault, oracle);

                let hermes = Hermes::default();
                let oracle = PythPullOracle::try_new(client)?;
                let pyth = PythPullOracleWithHermes::from_parts(client, &hermes, &oracle);
                let mut with_pyth = WithPullOracle::new(&pyth, builder, None).await?;

                let chainlink = if *oracle_testnet {
                    chainlink::Client::from_testnet_defaults()
                } else {
                    chainlink::Client::from_defaults()
                };

                match chainlink.as_ref() {
                    Ok(chainlink) => {
                        let factory = ChainlinkPullOracleFactory::new(store, 0).arced();
                        let oracle = factory.make_oracle(chainlink, client, true);

                        let (mut with_chainlink, feed_ids) =
                            WithPullOracle::from_consumer(oracle.clone(), with_pyth, None).await?;

                        let mut bundle = oracle
                            .prepare_feeds_bundle(
                                &feed_ids,
                                BundleOptions {
                                    max_packet_size: max_transaction_size,
                                    ..Default::default()
                                },
                            )
                            .await?;

                        bundle.append(
                            with_chainlink
                                .build_with_options(BundleOptions {
                                    max_packet_size: max_transaction_size,
                                    ..Default::default()
                                })
                                .await?,
                            false,
                        )?;

                        return crate::utils::send_or_serialize_bundle(
                            store,
                            bundle,
                            ctx,
                            serialize_only,
                            skip_preflight,
                            |signatures, error| {
                                match error {
                                    Some(err) => {
                                        tracing::error!(%err, "success txns: {signatures:#?}");
                                    }
                                    None => {
                                        tracing::info!("success txns: {signatures:#?}");
                                    }
                                }
                                Ok(())
                            },
                        )
                        .await;
                    }
                    Err(err) => {
                        tracing::warn!(%err, "failed to connect to the chainlink client");

                        let bundle = with_pyth
                            .build_with_options(BundleOptions {
                                max_packet_size: max_transaction_size,
                                ..Default::default()
                            })
                            .await?;

                        return crate::utils::send_or_serialize_bundle(
                            store,
                            bundle,
                            ctx,
                            serialize_only,
                            skip_preflight,
                            |signatures, error| {
                                match error {
                                    Some(err) => {
                                        tracing::error!(%err, "success txns: {signatures:#?}");
                                    }
                                    None => {
                                        tracing::info!("success txns: {signatures:#?}");
                                    }
                                }
                                Ok(())
                            },
                        )
                        .await;
                    }
                }
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
                let amount = match amount {
                    Some(amount) => *amount,
                    None => {
                        let vault = get_associated_token_address(&receiver, swap_in);
                        let account =
                            client
                                .account::<TokenAccount>(&vault)
                                .await?
                                .ok_or_else(|| {
                                    gmsol::Error::invalid_argument(
                                        "vault account is not initialized",
                                    )
                                })?;
                        account.amount
                    }
                };
                let (rpc, order) = client
                    .create_treasury_swap(
                        store,
                        market_token,
                        swap_in,
                        swap_out,
                        amount,
                        CreateTreasurySwapOptions {
                            swap_path: extra_swap_path.clone(),
                            min_swap_out_amount: *min_output_amount,
                            ..Default::default()
                        },
                    )
                    .await?
                    .swap_output(());
                println!("{order}");
                if let Some(lamports) = fund {
                    let swap_owner = client.find_treasury_receiver_address(&config);
                    let fund = client.transfer(&swap_owner, *lamports)?;
                    fund.merge(rpc)
                } else {
                    rpc
                }
            }
            Command::CancelSwap { order } => {
                client.cancel_treasury_swap(store, order, None).await?
            }
            Command::Receiver => {
                let config = client.find_treasury_config_address(store);
                let receiver = client.find_treasury_receiver_address(&config);
                println!("{receiver}");
                return Ok(());
            }
            Command::Withdraw {
                token,
                token_program_id,
                amount,
                decimals,
                target,
            } => {
                let target = target.unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(
                        &client.payer(),
                        token,
                        &token_program_id.unwrap_or(anchor_spl::token::ID),
                    )
                });
                client
                    .withdraw_from_treasury_vault(
                        store,
                        None,
                        token,
                        token_program_id.as_ref(),
                        *amount,
                        *decimals,
                        &target,
                    )
                    .await?
            }
        };
        crate::utils::send_or_serialize_transaction(
            store,
            req,
            ctx,
            serialize_only,
            skip_preflight,
            |signature| {
                tracing::info!("{signature}");
                Ok(())
            },
        )
        .await
    }
}
