use std::ops::Deref;

use eyre::OptionExt;
use gmsol_sdk::{
    builders::{token::WrapNative, NonceBytes},
    client::token_map::TokenMap,
    core::token_config::TokenMapAccess,
    ops::{
        exchange::{deposit, withdrawal},
        ExchangeOps,
    },
    programs::{anchor_lang::prelude::Pubkey, gmsol_store::accounts::Market},
    solana_utils::{
        instruction_group::GetInstructionsOptions,
        solana_sdk::{
            commitment_config::CommitmentConfig, instruction::Instruction, signer::Signer,
        },
    },
};

use super::utils::{Amount, GmAmount, Lamport, UsdValue};

/// Commands for exchange functionalities.
#[derive(Debug, clap::Args)]
pub struct Exchange {
    /// Nonce for actions.
    #[arg(long)]
    nonce: Option<NonceBytes>,
    /// Skips wrapping the native token when enabled.
    #[arg(long)]
    skip_native_wrap: bool,
    /// Commands for exchange functionalities.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Create a deposit.
    CreateDeposit {
        /// The address of the market token of the Market to deposit into.
        market_token: Pubkey,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
        /// Minimum amount of market tokens to mint.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        min_amount: GmAmount,
        /// The initial long token.
        #[arg(long, requires = "long_token_amount")]
        long_token: Option<Pubkey>,
        /// The initial short token.
        #[arg(long, requires = "short_token_amount")]
        short_token: Option<Pubkey>,
        /// The initial long token account.
        #[arg(long)]
        long_token_account: Option<Pubkey>,
        /// The initial short token account.
        #[arg(long)]
        short_token_account: Option<Pubkey>,
        /// The initial long token amount.
        #[arg(long, default_value_t = Amount::ZERO)]
        long_token_amount: Amount,
        /// The initial short token amount.
        #[arg(long, default_value_t = Amount::ZERO)]
        short_token_amount: Amount,
        /// Swap paths for long token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        long_swap: Vec<Pubkey>,
        /// Swap paths for short token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        short_swap: Vec<Pubkey>,
        /// Reciever.
        #[arg(long, group = "deposit_receiver")]
        receiver: Option<Pubkey>,
        #[arg(long, group = "deposit_receiver", requires = "min_amount")]
        first_deposit: bool,
    },
    /// Close a deposit account.
    CloseDeposit {
        /// The address of the deposit to cancel.
        deposit: Pubkey,
    },
    /// Create a withdrawal.
    CreateWithdrawal {
        /// The address of the market token of the Market to withdraw from.
        market_token: Pubkey,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
        /// The amount of market tokens to burn.
        #[arg(long)]
        amount: GmAmount,
        /// Final long token.
        #[arg(long)]
        long_token: Option<Pubkey>,
        /// Final short token.
        #[arg(long)]
        short_token: Option<Pubkey>,
        /// The market token account to use.
        #[arg(long)]
        market_token_account: Option<Pubkey>,
        /// The final long token account.
        #[arg(long)]
        long_token_account: Option<Pubkey>,
        /// The final short token account.
        #[arg(long)]
        short_token_account: Option<Pubkey>,
        /// Minimal amount of final long tokens to withdraw.
        #[arg(long, default_value_t = Amount::ZERO)]
        min_long_token_amount: Amount,
        /// Minimal amount of final short tokens to withdraw.
        #[arg(long, default_value_t = Amount::ZERO)]
        min_short_token_amount: Amount,
        /// Swap paths for long token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        long_swap: Vec<Pubkey>,
        /// Swap paths for short token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        short_swap: Vec<Pubkey>,
    },
    /// Close a withdrawal account.
    CloseWithdrawal {
        /// The address of the withdrawal to cancel.
        withdrawal: Pubkey,
    },
    /// Close an order.
    CloseOrder {
        /// The address of the order to cancel.
        order: Pubkey,
    },
    /// Create a market increase order.
    MarketIncrease {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Whether the collateral is long token.
        #[arg(long)]
        collateral_side: Side,
        /// Initial collateral token.
        #[arg(long, short = 'c')]
        initial_collateral_token: Option<Pubkey>,
        /// Initial collateral token account.
        #[arg(long, requires = "initial_collateral_token")]
        initial_collateral_token_account: Option<Pubkey>,
        /// Collateral amount.
        #[arg(long, short = 'a')]
        initial_collateral_token_amount: Amount,
        /// Position side.
        #[arg(long)]
        side: Side,
        /// Position increment size in usd.
        #[arg(long)]
        size: UsdValue,
        /// Swap paths for collateral token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Whether to wait for the action to be completed.
        #[arg(long, short)]
        wait: bool,
        /// Provide this to participate in a competition.
        #[arg(long)]
        competition: Option<Pubkey>,
    },
    /// Create a market decrese order.
    MarketDecrease {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Whether the collateral is long token.
        #[arg(long)]
        collateral_side: Side,
        /// Collateral withdrawal amount.
        #[arg(long, short = 'a', default_value_t = Amount::ZERO)]
        collateral_withdrawal_amount: Amount,
        /// Position side.
        #[arg(long)]
        side: Side,
        /// Position decrement size in usd.
        #[arg(long, default_value_t = UsdValue::ZERO)]
        size: UsdValue,
        /// Final output token.
        #[arg(long, short = 'o')]
        final_output_token: Option<Pubkey>,
        /// Swap paths for output token (collateral token).
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Whether to wait for the action to be completed.
        #[arg(long, short)]
        wait: bool,
        /// Provide this to participate in a competition.
        #[arg(long)]
        competition: Option<Pubkey>,
    },
}

impl super::Command for Exchange {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let nonce = self.nonce.map(|nonce| nonce.to_bytes());
        let store = ctx.store();
        let client = ctx.client()?;
        let token_map = match &self.command {
            Command::CloseOrder { .. }
            | Command::CloseDeposit { .. }
            | Command::CloseWithdrawal { .. } => None,
            Command::CreateWithdrawal {
                min_long_token_amount,
                min_short_token_amount,
                ..
            } if min_long_token_amount.is_zero() && min_short_token_amount.is_zero() => None,
            Command::MarketDecrease {
                collateral_withdrawal_amount,
                ..
            } if collateral_withdrawal_amount.is_zero() => None,
            _ => Some(client.authorized_token_map(store).await?),
        };
        let options = ctx.bundle_options();
        let mut collector = (!self.skip_native_wrap).then(NativeCollector::default);
        let owner = &client.payer();
        let bundle = match &self.command {
            Command::CreateDeposit {
                market_token,
                extra_execution_fee,
                min_amount,
                long_token,
                short_token,
                long_token_account,
                short_token_account,
                long_token_amount,
                short_token_amount,
                long_swap,
                short_swap,
                receiver,
                first_deposit,
            } => {
                let market_address = client.find_market_address(store, market_token);
                let market = client.market(&market_address).await?;
                let mut builder = client.create_deposit(store, market_token);
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if !long_token_amount.is_zero() {
                    let long_token_amount = token_amount(
                        long_token_amount,
                        long_token.as_ref(),
                        token_map.as_ref().expect("must exist"),
                        &market,
                        true,
                    )?;
                    builder.long_token(
                        long_token_amount,
                        long_token.as_ref(),
                        long_token_account.as_ref(),
                    );
                    if let Some(c) = collector.as_mut() {
                        c.add(
                            long_token_amount,
                            owner,
                            long_token.as_ref(),
                            long_token_account.as_ref(),
                            &market,
                            true,
                        )?;
                    }
                }
                if !short_token_amount.is_zero() {
                    let short_token_amount = token_amount(
                        short_token_amount,
                        short_token.as_ref(),
                        token_map.as_ref().expect("must exist"),
                        &market,
                        false,
                    )?;
                    builder.short_token(
                        short_token_amount,
                        short_token.as_ref(),
                        short_token_account.as_ref(),
                    );
                    if let Some(c) = collector.as_mut() {
                        c.add(
                            short_token_amount,
                            owner,
                            short_token.as_ref(),
                            short_token_account.as_ref(),
                            &market,
                            false,
                        )?;
                    }
                }
                let receiver = if *first_deposit {
                    Some(client.find_first_deposit_owner_address())
                } else {
                    *receiver
                };
                let (builder, deposit) = builder
                    .execution_fee(extra_execution_fee.to_u64()? + deposit::MIN_EXECUTION_LAMPORTS)
                    .min_market_token(min_amount.to_u64()?)
                    .long_token_swap_path(long_swap.clone())
                    .short_token_swap_path(short_swap.clone())
                    .receiver(receiver)
                    .build_with_address()
                    .await?;
                println!("Deposit: {deposit}");
                builder
                    .pre_instructions(
                        collector
                            .as_ref()
                            .map(|c| c.to_instructions(owner))
                            .transpose()?
                            .unwrap_or_default(),
                        false,
                    )
                    .into_bundle_with_options(options)?
            }
            Command::CloseDeposit { deposit } => client
                .close_deposit(store, deposit)
                .build()
                .await?
                .into_bundle_with_options(options)?,

            Command::CreateWithdrawal {
                market_token,
                extra_execution_fee,
                amount,
                long_token,
                short_token,
                market_token_account,
                long_token_account,
                short_token_account,
                min_long_token_amount,
                min_short_token_amount,
                long_swap,
                short_swap,
            } => {
                let mut builder = client.create_withdrawal(store, market_token, amount.to_u64()?);
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(account) = market_token_account {
                    builder.market_token_account(account);
                }
                if let Some(token) = long_token {
                    builder.final_long_token(token, long_token_account.as_ref());
                }
                if let Some(token) = short_token {
                    builder.final_short_token(token, short_token_account.as_ref());
                }
                let (min_long_token_amount, min_short_token_amount) =
                    if min_long_token_amount.is_zero() && min_short_token_amount.is_zero() {
                        (0, 0)
                    } else {
                        let market_address = client.find_market_address(store, market_token);
                        let market = client.market(&market_address).await?;
                        (
                            token_amount(
                                min_long_token_amount,
                                long_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                &market,
                                true,
                            )?,
                            token_amount(
                                min_short_token_amount,
                                short_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                &market,
                                false,
                            )?,
                        )
                    };
                let (builder, withdrawal) = builder
                    .execution_fee(
                        extra_execution_fee.to_u64()? + withdrawal::MIN_EXECUTION_LAMPORTS,
                    )
                    .min_final_long_token_amount(min_long_token_amount)
                    .min_final_short_token_amount(min_short_token_amount)
                    .long_token_swap_path(long_swap.clone())
                    .short_token_swap_path(short_swap.clone())
                    .build_with_address()
                    .await?;
                println!("Withdrawal: {withdrawal}");
                builder.into_bundle_with_options(options)?
            }
            Command::CloseWithdrawal { withdrawal } => client
                .close_withdrawal(store, withdrawal)
                .build()
                .await?
                .into_bundle_with_options(options)?,
            Command::CloseOrder { order } => client
                .close_order(order)?
                .build()
                .await?
                .into_bundle_with_options(options)?,
            Command::MarketIncrease {
                market_token,
                collateral_side,
                initial_collateral_token,
                initial_collateral_token_account,
                initial_collateral_token_amount,
                side,
                size,
                swap,
                wait,
                competition,
            } => {
                let market_address = client.find_market_address(store, market_token);
                let market = client.market(&market_address).await?;
                let is_collateral_token_long = collateral_side.is_long();
                let initial_collateral_token_amount = token_amount(
                    initial_collateral_token_amount,
                    initial_collateral_token.as_ref(),
                    token_map.as_ref().expect("must exist"),
                    &market,
                    is_collateral_token_long,
                )?;
                if let Some(c) = collector.as_mut() {
                    c.add(
                        initial_collateral_token_amount,
                        owner,
                        initial_collateral_token.as_ref(),
                        initial_collateral_token_account.as_ref(),
                        &market,
                        is_collateral_token_long,
                    )?;
                }
                let mut builder = client.market_increase(
                    store,
                    market_token,
                    is_collateral_token_long,
                    initial_collateral_token_amount,
                    side.is_long(),
                    size.to_u128()?,
                );
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(token) = initial_collateral_token {
                    builder
                        .initial_collateral_token(token, initial_collateral_token_account.as_ref());
                }

                builder.swap_path(swap.clone());

                if let Some(competition) = competition {
                    builder.competition(competition);
                }

                let (rpc, order) = builder.build_with_address().await?;

                let rpc = rpc.pre_instructions(
                    collector
                        .as_ref()
                        .map(|c| c.to_instructions(owner))
                        .transpose()?
                        .unwrap_or_default(),
                    false,
                );

                println!("Order: {order}");

                let tx = if *wait {
                    ctx.require_not_serialize_only_mode()?;
                    ctx.require_not_ix_buffer_mode()?;

                    let signature = rpc.send_without_preflight().await?;
                    tracing::info!("created a market increase order {order} at tx {signature}");

                    wait_for_order(client, &order).await?;
                    return Ok(());
                } else {
                    rpc
                };
                tx.into_bundle_with_options(options)?
            }
            Command::MarketDecrease {
                market_token,
                collateral_side,
                collateral_withdrawal_amount,
                side,
                size,
                final_output_token,
                swap,
                wait,
                competition,
            } => {
                let is_collateral_token_long = collateral_side.is_long();
                let collateral_withdrawal_amount = if collateral_withdrawal_amount.is_zero() {
                    0
                } else {
                    let market_address = client.find_market_address(store, market_token);
                    let market = client.market(&market_address).await?;
                    token_amount(
                        collateral_withdrawal_amount,
                        final_output_token.as_ref(),
                        token_map.as_ref().expect("must exist"),
                        &market,
                        is_collateral_token_long,
                    )?
                };
                let mut builder = client.market_decrease(
                    store,
                    market_token,
                    is_collateral_token_long,
                    collateral_withdrawal_amount,
                    side.is_long(),
                    size.to_u128()?,
                );
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(token) = final_output_token {
                    builder.final_output_token(token);
                }
                builder.swap_path(swap.clone());

                if let Some(competition) = competition {
                    builder.competition(competition);
                }

                let (rpc, order) = builder.build_with_address().await?;

                println!("Order: {order}");

                let tx = if *wait {
                    ctx.require_not_serialize_only_mode()?;
                    ctx.require_not_ix_buffer_mode()?;

                    let signature = rpc.send_without_preflight().await?;
                    tracing::info!("created a market decrease order {order} at tx {signature}");

                    wait_for_order(client, &order).await?;
                    return Ok(());
                } else {
                    rpc
                };
                tx.into_bundle_with_options(options)?
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

fn token_amount(
    amount: &Amount,
    token: Option<&Pubkey>,
    token_map: &TokenMap,
    market: &Market,
    is_long: bool,
) -> eyre::Result<u64> {
    let token = match token {
        Some(token) => token,
        None => {
            if is_long {
                &market.meta.long_token_mint
            } else {
                &market.meta.short_token_mint
            }
        }
    };
    let decimals = token_map
        .get(token)
        .ok_or_eyre("token config not found")?
        .token_decimals;
    Ok(amount.to_u64(decimals)?)
}

async fn wait_for_order<C: Deref<Target = impl Signer> + Clone>(
    client: &gmsol_sdk::Client<C>,
    order: &Pubkey,
) -> gmsol_sdk::Result<()> {
    let trade = client
        .complete_order(order, Some(CommitmentConfig::confirmed()))
        .await?;
    match trade {
        Some(trade) => {
            tracing::info!(%order, "order completed with trade event: {trade:#?}");
        }
        None => {
            tracing::warn!(%order, "order completed without trade event");
        }
    }
    Ok(())
}

#[derive(Default)]
struct NativeCollector {
    lamports: u64,
}

impl NativeCollector {
    fn add(
        &mut self,
        amount: u64,
        owner: &Pubkey,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
        market: &Market,
        is_long: bool,
    ) -> eyre::Result<()> {
        use anchor_spl::{
            associated_token::get_associated_token_address, token::spl_token::native_mint::ID,
        };

        let token = match token {
            Some(token) => token,
            None => {
                if is_long {
                    &market.meta.long_token_mint
                } else {
                    &market.meta.short_token_mint
                }
            }
        };

        if *token == ID {
            if let Some(token_account) = token_account {
                let expected_account = get_associated_token_address(owner, token);
                if expected_account != *token_account {
                    eyre::bail!("wrapping native token requires an associated token account");
                }
            }
            self.lamports += amount;
        }

        Ok(())
    }

    fn to_instructions(&self, owner: &Pubkey) -> eyre::Result<Vec<Instruction>> {
        use gmsol_sdk::IntoAtomicGroup;

        Ok(WrapNative::builder()
            .lamports(self.lamports)
            .owner(*owner)
            .build()
            .into_atomic_group(&false)?
            .instructions_with_options(GetInstructionsOptions {
                without_compute_budget: true,
                ..Default::default()
            })
            .map(|ix| (*ix).clone())
            .collect())
    }
}
