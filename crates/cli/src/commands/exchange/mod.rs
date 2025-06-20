#[cfg(feature = "execute")]
pub(crate) mod executor;

use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use clap::ArgGroup;
use eyre::OptionExt;
use gmsol_sdk::{
    builders::{token::WrapNative, NonceBytes},
    constants::MARKET_DECIMALS,
    core::{market::MarketMeta, order::OrderKind},
    decode::gmsol::programs::GMSOLAccountData,
    ops::{
        exchange::{deposit, glv_deposit, glv_shift, glv_withdrawal, shift, withdrawal},
        AddressLookupTableOps, ExchangeOps,
    },
    programs::{
        anchor_lang::prelude::Pubkey,
        gmsol_store::{
            accounts::Market,
            types::{DecreasePositionSwapType, UpdateOrderParams},
        },
    },
    serde::{serde_market::SerdeMarket, serde_position::SerdePosition, StringPubkey},
    solana_utils::{
        instruction_group::GetInstructionsOptions,
        solana_sdk::{
            commitment_config::CommitmentConfig, instruction::Instruction, signer::Signer,
        },
    },
    utils::{Amount, GmAmount, Lamport, Value},
};
use indexmap::IndexMap;

use crate::{
    commands::utils::{get_token_amount_with_token_map, token_amount, unit_price},
    config::DisplayOptions,
};

use super::{
    glv::GlvToken,
    utils::{price_to_min_output_amount, Side},
};

/// Exchange-related commands.
#[derive(Debug, clap::Args)]
pub struct Exchange {
    /// Nonce for actions.
    #[arg(long)]
    nonce: Option<NonceBytes>,
    /// Skips wrapping the native token when enabled.
    #[arg(long)]
    skip_native_wrap: bool,
    /// Commands.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Fetches market accounts.
    Markets {
        #[arg(group = "market-address")]
        market_token: Option<StringPubkey>,
        #[arg(long, group = "market-address")]
        address: Option<StringPubkey>,
    },
    /// Fetches actions or positions.
    #[command(group(ArgGroup::new("select-action").required(true)))]
    Actions {
        #[arg(group = "select-action")]
        address: Option<Pubkey>,
        #[arg(long, group = "select-owner")]
        owner: Option<Pubkey>,
        #[arg(long, group = "select-owner")]
        all: bool,
        /// Provides to include empty positions / actions.
        #[arg(long)]
        include_empty: bool,
        #[arg(long)]
        market_token: Option<Pubkey>,
        #[arg(long, group = "select-action")]
        orders: bool,
        #[arg(long, group = "select-action")]
        positions: bool,
        #[arg(long, group = "select-action")]
        deposits: bool,
        #[arg(long, group = "select-action")]
        withdrawals: bool,
        #[arg(long, group = "select-action")]
        shifts: bool,
        #[arg(long, group = "select-action")]
        glv_deposits: bool,
        #[arg(long, group = "select-action")]
        glv_withdrawals: bool,
        #[arg(long, group = "select-action")]
        glv_shifts: bool,
    },
    /// Creates a deposit.
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
        /// The address of the deposit to close.
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
        /// The address of the withdrawal to close.
        withdrawal: Pubkey,
    },
    /// Create a shift.
    CreateShift {
        /// From market token.
        #[arg(long, value_name = "FROM_MARKET_TOKEN")]
        from: Pubkey,
        /// To market token.
        #[arg(long, value_name = "TO_MARKET_TOKEN")]
        to: Pubkey,
        /// Amount.
        #[arg(long)]
        amount: GmAmount,
        /// Min output amount.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        min_output_amount: GmAmount,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
    },
    /// Close a shift.
    CloseShift {
        /// The address of the shift to close.
        shift: Pubkey,
    },
    /// Close an order.
    CloseOrder {
        /// The address of the order to close.
        order: Pubkey,
        /// Whether to skip callback.
        skip_callabck: bool,
    },
    /// Create a market increase order.
    MarketIncrease {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Whether the collateral is long token.
        #[arg(long)]
        collateral_side: Side,
        /// Min collateral amount.
        #[arg(long)]
        min_collateral_amount: Option<Amount>,
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
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        /// Position increment size in usd.
        #[arg(long)]
        size: Value,
        /// Swap paths for collateral token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Whether to wait for the action to be completed.
        #[arg(long, short)]
        wait: bool,
        /// Provide this to participate in a competition.
        #[arg(long)]
        competition: Option<Pubkey>,
        /// Provide to only prepare position for the order.
        #[arg(long)]
        prepare_position_only: bool,
    },
    /// Create a limit increase order.
    LimitIncrease {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Whether the collateral is long token.
        #[arg(long)]
        collateral_side: Side,
        /// Min collateral amount.
        #[arg(long)]
        min_collateral_amount: Option<Amount>,
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
        /// Trigger price.
        #[arg(long)]
        price: Value,
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        /// Position increment size in usd.
        #[arg(long)]
        size: Value,
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
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        /// Position decrement size in usd.
        #[arg(long, default_value_t = Value::ZERO)]
        size: Value,
        /// Final output token.
        #[arg(long, short = 'c')]
        final_output_token: Option<Pubkey>,
        /// Min output value.
        #[arg(long)]
        min_output: Option<Value>,
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
    /// Create a limit decrese order.
    LimitDecrease {
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
        /// Trigger price.
        #[arg(long)]
        price: Value,
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        /// Position decrement size in usd.
        #[arg(long, default_value_t = Value::ZERO)]
        size: Value,
        /// Final output token.
        #[arg(long, short = 'c')]
        final_output_token: Option<Pubkey>,
        /// Min output value.
        #[arg(long)]
        min_output: Option<Value>,
        /// Swap paths for output token (collateral token).
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Whether to wait for the action to be completed.
        #[arg(long, short)]
        wait: bool,
        /// Provide this to participate in a competition.
        #[arg(long)]
        competition: Option<Pubkey>,
        /// Valid from this timestamp.
        #[arg(long)]
        valid_from_ts: Option<humantime::Timestamp>,
    },
    /// Create a stop-loss decrese order.
    StopLoss {
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
        /// Trigger price.
        #[arg(long)]
        price: Value,
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        #[arg(long, default_value_t = Value::ZERO)]
        size: Value,
        /// Final output token.
        #[arg(long, short = 'c')]
        final_output_token: Option<Pubkey>,
        /// Min output value.
        #[arg(long)]
        min_output: Option<Value>,
        /// Swap paths for output token (collateral token).
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Whether to wait for the action to be completed.
        #[arg(long, short)]
        wait: bool,
        /// Provide this to participate in a competition.
        #[arg(long)]
        competition: Option<Pubkey>,
        /// Valid from this timestamp.
        #[arg(long)]
        valid_from_ts: Option<humantime::Timestamp>,
    },
    /// Update a limit or stop-loss order.
    UpdateOrder {
        /// The address of the swap order to update.
        address: Pubkey,
        /// New Tigger price.
        #[arg(long)]
        price: Option<Value>,
        /// Acceptable price.
        #[arg(long)]
        acceptable_price: Option<Value>,
        /// Min output amount or value.
        #[arg(long)]
        min_output: Option<Amount>,
        /// New size.
        #[arg(long)]
        size: Option<Value>,
        /// Valid from this timestamp.
        #[arg(long)]
        valid_from_ts: Option<humantime::Timestamp>,
    },
    /// Create a market swap order.
    MarketSwap {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Output side.
        #[arg(long, short = 'o')]
        output_side: Side,
        /// Initial swap in token.
        #[arg(long, short = 'i')]
        initial_swap_in_token: Pubkey,
        /// Initial swap in token account.
        #[arg(long)]
        initial_swap_in_token_account: Option<Pubkey>,
        /// Collateral amount.
        #[arg(long, short = 'a')]
        initial_swap_in_token_amount: Amount,
        /// Extra swap path. No need to provide the target market token;
        /// it will be automatically added to the end of the swap path.
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Min output amount.
        #[arg(long)]
        min_output_amount: Option<Amount>,
    },
    /// Create a limit swap order.
    LimitSwap {
        /// The address of the market token of the position's market.
        market_token: Pubkey,
        /// Output side.
        #[arg(long)]
        output_side: Side,
        /// Limit price (`token_in` to `token_out` price)
        #[arg(long)]
        price: Value,
        /// Initial swap in token.
        #[arg(long, short = 'i')]
        initial_swap_in_token: Pubkey,
        /// Initial swap in token account.
        #[arg(long)]
        initial_swap_in_token_account: Option<Pubkey>,
        /// Collateral amount.
        #[arg(long, short = 'a')]
        initial_swap_in_token_amount: Amount,
        /// Extra swap path. No need to provide the target market token;
        /// it will be automatically added to the end of the swap path.
        #[arg(long, short, action = clap::ArgAction::Append)]
        swap: Vec<Pubkey>,
        /// Valid from this timestamp.
        #[arg(long)]
        valid_from_ts: Option<humantime::Timestamp>,
    },
    /// Update a limit swap order.
    UpdateSwap {
        /// The address of the swap order to update.
        address: Pubkey,
        /// New limit price (`token_in` to `token_out` price).
        #[arg(long)]
        price: Option<Value>,
        /// Valid from this timestamp.
        #[arg(long)]
        valid_from_ts: Option<humantime::Timestamp>,
    },
    /// Cancel an order if no position.
    /// Requires appropriate permissions.
    CancelOrderIfNoPosition {
        order: Pubkey,
        #[arg(long)]
        keep: bool,
    },
    /// GLV operations.
    Glv {
        #[command(flatten)]
        glv_token: GlvToken,
        #[command(subcommand)]
        command: GlvCommand,
    },
    /// Executes the given action.
    /// Requires appropriate permissions.
    #[cfg(feature = "execute")]
    Execute {
        #[command(flatten)]
        args: executor::ExecutorArgs,
        address: Pubkey,
        #[arg(long)]
        skip_close: bool,
        #[arg(long)]
        throw_error_on_failure: bool,
    },
    /// Update the ADL state for the given market.
    /// Requires appropriate permissions.
    #[cfg(feature = "execute")]
    UpdateAdl {
        #[command(flatten)]
        args: executor::ExecutorArgs,
        market_token: Pubkey,
        /// Provides to only update for one side.
        #[arg(long, short)]
        side: Option<Side>,
    },
    /// Close a profitable position when ADL is enabled.
    #[cfg(feature = "execute")]
    Adl {
        #[command(flatten)]
        args: executor::ExecutorArgs,
        #[clap(requires = "close_size")]
        position: Pubkey,
        /// The size to be closed.
        #[arg(long, group = "close_size")]
        size: Option<u128>,
        #[arg(long, group = "close_size")]
        close_all: bool,
    },
    /// Liquidate a position.
    #[cfg(feature = "execute")]
    Liquidate {
        #[command(flatten)]
        args: executor::ExecutorArgs,
        position: Pubkey,
    },
}

#[derive(Debug, clap::Subcommand)]
enum GlvCommand {
    /// Create a GLV deposit.
    CreateDeposit {
        /// The address of the market token of the GLV Market to deposit into.
        market_token: Pubkey,
        #[arg(long, group = "deposit-receiver")]
        receiver: Option<Pubkey>,
        #[arg(long, group = "deposit-receiver", requires = "min_amount")]
        first_deposit: bool,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
        /// Minimum amount of GLV tokens to mint.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        min_amount: GmAmount,
        /// Minimum amount of market tokens to mint.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        min_market_token_amount: GmAmount,
        /// The initial long token.
        #[arg(long, requires = "long_token_amount")]
        long_token: Option<Pubkey>,
        /// The initial short token.
        #[arg(long, requires = "short_token_amount")]
        short_token: Option<Pubkey>,
        /// The market token account.
        market_token_account: Option<Pubkey>,
        /// The initial long token account.
        #[arg(long)]
        long_token_account: Option<Pubkey>,
        /// The initial short token account.
        #[arg(long)]
        short_token_account: Option<Pubkey>,
        /// The initial long token amount.
        /// Market token amount to deposit.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        market_token_amount: GmAmount,
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
    },
    /// Close a GLV deposit.
    CloseDeposit {
        /// The address of the GLV deposit to close.
        glv_deposit: Pubkey,
    },
    /// Create a GLV withdrawal.
    CreateWithdrawal {
        /// The address of the market token of the GLV Market to withdraw from.
        market_token: Pubkey,
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
        /// The amount of GLV tokens to burn.
        #[arg(long)]
        amount: GmAmount,
        /// Final long token.
        #[arg(long)]
        final_long_token: Option<Pubkey>,
        /// Final short token.
        #[arg(long)]
        final_short_token: Option<Pubkey>,
        /// The GLV token account to use.
        #[arg(long)]
        glv_token_account: Option<Pubkey>,
        /// Minimal amount of final long tokens to withdraw.
        #[arg(long, default_value_t = Amount::ZERO)]
        min_final_long_token_amount: Amount,
        /// Minimal amount of final short tokens to withdraw.
        #[arg(long, default_value_t = Amount::ZERO)]
        min_final_short_token_amount: Amount,
        /// Swap paths for long token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        long_swap: Vec<Pubkey>,
        /// Swap paths for short token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        short_swap: Vec<Pubkey>,
    },
    /// Close a GLV withdrawal.
    CloseWithdrawal {
        /// The address of the GLV withdrawal to close.
        glv_withdrawal: Pubkey,
    },
    /// Create a GLV shift.
    /// Requires appropriate permissions.
    CreateShift {
        /// From market token.
        #[arg(long, value_name = "FROM_MARKET_TOKEN")]
        from: Pubkey,
        /// To market token.
        #[arg(long, value_name = "TO_MARKET_TOKEN")]
        to: Pubkey,
        /// Amount.
        #[arg(long)]
        amount: GmAmount,
        /// Min output amount.
        #[arg(long, default_value_t = GmAmount::ZERO)]
        min_output_amount: GmAmount,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = Lamport::ZERO)]
        extra_execution_fee: Lamport,
    },
    /// Close a GLV shift.
    /// Requires appropriate permissions.
    CloseShift {
        /// The address of the GLV shift to close.
        glv_shift: Pubkey,
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
        let mut token_map = match &self.command {
            Command::CloseOrder { .. }
            | Command::CloseDeposit { .. }
            | Command::CloseWithdrawal { .. }
            | Command::CreateShift { .. }
            | Command::CloseShift { .. } => None,
            Command::CreateWithdrawal {
                min_long_token_amount,
                min_short_token_amount,
                ..
            } if min_long_token_amount.is_zero() && min_short_token_amount.is_zero() => None,
            Command::MarketDecrease {
                collateral_withdrawal_amount,
                acceptable_price,
                ..
            } if collateral_withdrawal_amount.is_zero() && acceptable_price.is_none() => None,
            Command::UpdateOrder {
                price,
                acceptable_price,
                ..
            } if price.is_none() && acceptable_price.is_none() => None,
            Command::UpdateSwap { price, .. } if price.is_none() => None,
            Command::Glv { command, .. } => match command {
                GlvCommand::CloseDeposit { .. }
                | GlvCommand::CloseWithdrawal { .. }
                | GlvCommand::CloseShift { .. }
                | GlvCommand::CreateShift { .. } => None,
                GlvCommand::CreateDeposit {
                    long_token_amount,
                    short_token_amount,
                    ..
                } if long_token_amount.is_zero() && short_token_amount.is_zero() => None,
                GlvCommand::CreateWithdrawal {
                    min_final_long_token_amount,
                    min_final_short_token_amount,
                    ..
                } if min_final_long_token_amount.is_zero()
                    && min_final_short_token_amount.is_zero() =>
                {
                    None
                }
                _ => Some(client.authorized_token_map(store).await?),
            },
            _ => Some(client.authorized_token_map(store).await?),
        };
        let options = ctx.bundle_options();
        let mut collector = (!self.skip_native_wrap).then(NativeCollector::default);
        let owner = &client.payer();
        let output = ctx.config().output();
        let bundle = match &self.command {
            Command::Markets {
                market_token,
                address,
            } => {
                let token_map = token_map.as_ref().expect("must exist");
                if address.is_none() && market_token.is_none() {
                    let markets = client.markets(store).await?;
                    let mut serde_markets = markets
                        .iter()
                        .map(|(p, m)| SerdeMarket::from_market(m, token_map).map(|m| (p, m)))
                        .collect::<gmsol_sdk::Result<Vec<(_, _)>>>()?;
                    serde_markets.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
                    serde_markets.sort_by_key(|(_, m)| m.enabled);
                    println!(
                        "{}",
                        output.display_keyed_accounts(
                            serde_markets,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("meta.market_token", "Market Token"),
                                ("enabled", "Is Enabled"),
                                ("state.long_token_balance", "◎ Long Token"),
                                ("state.short_token_balance", "◎ Short Token"),
                                ("pools.claimable_fee.long_amount", "◎ Claimable Long Token"),
                                (
                                    "pools.claimable_fee.short_amount",
                                    "◎ Claimable Short Token"
                                ),
                            ]),
                        )?
                    );
                } else {
                    let address = if let Some(address) = address {
                        **address
                    } else if let Some(market_token) = market_token {
                        client.find_market_address(store, market_token)
                    } else {
                        unreachable!()
                    };
                    let market = client.market(&address).await?;
                    let market = SerdeMarket::from_market(&market, token_map)?;
                    println!(
                        "{}",
                        output.display_keyed_account(
                            &address,
                            market,
                            DisplayOptions::table_projection([
                                ("name", "Name"),
                                ("pubkey", "Address"),
                                ("meta.market_token", "Market Token"),
                                ("meta.index_token", "Index Token"),
                                ("meta.long_token", "Long Token"),
                                ("meta.short_token", "Short Token"),
                                ("enabled", "Is Enabled"),
                                ("is_pure", "Is Pure"),
                                ("is_adl_enabled_for_long", "Is ADL Enabled (Long)"),
                                ("is_adl_enabled_for_short", "Is ADL Enabled (Short)"),
                                ("is_gt_minting_enabled", "Is GT Minting Enabled"),
                                ("state.long_token_balance", "◎ Long Token"),
                                ("state.short_token_balance", "◎ Short Token"),
                                ("state.funding_factor_per_second", "Funding Factor"),
                                (
                                    "pools.open_interest_for_long.long_amount",
                                    "Long OI (Long Token)"
                                ),
                                (
                                    "pools.open_interest_for_long.short_amount",
                                    "Long OI (Short Token)"
                                ),
                                (
                                    "pools.open_interest_for_short.long_amount",
                                    "Short OI (Long Token)"
                                ),
                                (
                                    "pools.open_interest_for_short.short_amount",
                                    "Short OI (Short Token)"
                                )
                            ])
                        )?
                    );
                }
                return Ok(());
            }
            Command::Actions {
                address,
                owner,
                all,
                include_empty,
                market_token,
                orders,
                positions,
                deposits,
                withdrawals,
                shifts,
                glv_deposits,
                glv_withdrawals,
                glv_shifts,
            } => {
                let owner = (!*all).then(|| owner.as_ref().copied().unwrap_or(client.payer()));
                if let Some(address) = address {
                    let decoded = client
                        .decode_account_with_config(address, Default::default())
                        .await?
                        .into_value()
                        .ok_or_eyre("account not found")?;
                    match decoded {
                        GMSOLAccountData::Position(position) => {
                            let market_address =
                                client.find_market_address(store, &position.market_token);
                            let market = client.market(&market_address).await?;
                            let position = SerdePosition::from_position(
                                &position,
                                &market.meta.into(),
                                token_map.as_ref().expect("must exist"),
                            )?;
                            println!(
                                "{}",
                                output.display_keyed_account(
                                    address,
                                    position,
                                    DisplayOptions::table_projection([
                                        ("kind", "Kind"),
                                        ("pubkey", "Address"),
                                        ("owner", "Owner"),
                                        ("is_long", "Is Long"),
                                        ("market_token", "Market Token"),
                                        ("collateral_token", "Collateral Token"),
                                        ("state.trade_id", "Last Trade ID"),
                                        ("state.updated_at_slot", "Last Updated Slot"),
                                        ("state.increased_at", "Last Increased At"),
                                        ("state.decreased_at", "Last Decreased At"),
                                        ("state.collateral_amount", "Collateral Amount"),
                                        ("state.size_in_usd", "$ Size"),
                                        ("state.size_in_tokens", "◎ Size In Tokens"),
                                    ])
                                    .add_extra(
                                        serde_json::json!({
                                            "kind": "Position",
                                        })
                                    )?
                                )?
                            );
                        }
                        decoded => {
                            println!("{decoded:#?}");
                        }
                    }
                } else if *orders {
                    let orders = client
                        .orders(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{orders:?}");
                } else if *positions {
                    let token_map = token_map.as_ref().expect("must exist");
                    let positions = client
                        .positions(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    let market_tokens = positions
                        .values()
                        .filter(|p| *include_empty || p.state.size_in_usd != 0)
                        .map(|p| p.market_token)
                        .collect::<HashSet<_>>();
                    let mut market_metas = HashMap::<_, _>::default();
                    for market_token in market_tokens {
                        let market = client
                            .market(&client.find_market_address(store, &market_token))
                            .await?;
                        market_metas.insert(market_token, MarketMeta::from(market.meta));
                    }
                    let mut positions = positions
                        .iter()
                        .filter(|(_, p)| *include_empty || p.state.size_in_usd != 0)
                        .map(|(k, p)| {
                            Ok((
                                *k,
                                SerdePosition::from_position(
                                    p,
                                    market_metas.get(&p.market_token).unwrap(),
                                    token_map,
                                )?,
                            ))
                        })
                        .collect::<eyre::Result<IndexMap<_, _>>>()?;
                    positions.sort_by(|_, a, _, b| {
                        a.state.size_in_usd.cmp(&b.state.size_in_usd).reverse()
                    });
                    positions.sort_by(|_, a, _, b| a.market_token.cmp(&b.market_token));
                    let output = output.display_keyed_accounts(
                        positions,
                        DisplayOptions::table_projection([
                            ("pubkey", "Address"),
                            ("market_token", "Market Token"),
                            ("is_long", "Is Long"),
                            ("is_collateral_long_token", "Is Collateral Long"),
                            ("state.collateral_amount", "Collateral Amount"),
                            ("state.size_in_usd", "Size($)"),
                            ("state.trade_id", "Last Trade ID"),
                        ])
                        .set_empty_message("No Positions"),
                    )?;
                    println!("{output}");
                } else if *deposits {
                    let deposits = client
                        .deposits(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{deposits:?}");
                } else if *withdrawals {
                    let withdrawals = client
                        .withdrawals(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{withdrawals:?}");
                } else if *shifts {
                    let shifts = client
                        .shifts(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{shifts:?}");
                } else if *glv_deposits {
                    let glv_deposits = client
                        .glv_deposits(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{glv_deposits:?}");
                } else if *glv_withdrawals {
                    let glv_withdrawals = client
                        .glv_withdrawals(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{glv_withdrawals:?}");
                } else if *glv_shifts {
                    let glv_shifts = client
                        .glv_shifts(store, owner.as_ref(), market_token.as_ref())
                        .await?;
                    println!("{glv_shifts:?}");
                }

                return Ok(());
            }
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
            Command::CreateShift {
                from,
                to,
                amount,
                min_output_amount,
                extra_execution_fee,
            } => {
                let mut builder = client.create_shift(store, from, to, amount.to_u64()?);
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                builder
                    .execution_fee(extra_execution_fee.to_u64()? + shift::MIN_EXECUTION_LAMPORTS)
                    .min_to_market_token_amount(min_output_amount.to_u64()?);

                let (rpc, shift) = builder.build_with_address()?;

                println!("Shift: {shift}");

                rpc.into_bundle_with_options(options)?
            }
            Command::CloseShift { shift } => client
                .close_shift(shift)
                .build()
                .await?
                .into_bundle_with_options(options)?,
            Command::CloseOrder {
                order,
                skip_callabck,
            } => client
                .close_order(order)?
                .skip_callback(*skip_callabck)
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
                min_collateral_amount,
                acceptable_price,
                prepare_position_only,
            } => {
                let market_address = client.find_market_address(store, market_token);
                let market = client.market(&market_address).await?;
                let token_map = token_map.as_ref().expect("must exist");
                let is_collateral_token_long = collateral_side.is_long();
                let initial_collateral_token_amount = token_amount(
                    initial_collateral_token_amount,
                    initial_collateral_token.as_ref(),
                    token_map,
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
                if let Some(amount) = min_collateral_amount {
                    builder.min_output_amount(
                        token_amount(amount, None, token_map, &market, is_collateral_token_long)?
                            .into(),
                    );
                }
                if let Some(price) = acceptable_price {
                    builder.acceptable_price(unit_price(price, token_map, &market)?);
                }

                builder.swap_path(swap.clone());

                if let Some(competition) = competition {
                    builder.competition(competition);
                }

                if *prepare_position_only {
                    let (rpc, position) = builder.build_prepare_position().await?.swap_output(());
                    println!("Position: {position}");
                    rpc.into_bundle_with_options(options)?
                } else {
                    for alt in ctx.config().alts() {
                        let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                        builder.add_alt(alt);
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
            }
            Command::LimitIncrease {
                market_token,
                collateral_side,
                initial_collateral_token,
                initial_collateral_token_account,
                initial_collateral_token_amount,
                side,
                price,
                size,
                swap,
                wait,
                competition,
                min_collateral_amount,
                acceptable_price,
            } => {
                let market_address = client.find_market_address(store, market_token);
                let market = client.market(&market_address).await?;
                let token_map = token_map.as_ref().expect("must exist");
                let price = unit_price(price, token_map, &market)?;
                let is_collateral_token_long = collateral_side.is_long();
                let initial_collateral_token_amount = token_amount(
                    initial_collateral_token_amount,
                    initial_collateral_token.as_ref(),
                    token_map,
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
                let mut builder = client.limit_increase(
                    store,
                    market_token,
                    side.is_long(),
                    size.to_u128()?,
                    price,
                    is_collateral_token_long,
                    initial_collateral_token_amount,
                );
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(token) = initial_collateral_token {
                    builder
                        .initial_collateral_token(token, initial_collateral_token_account.as_ref());
                }

                if let Some(competition) = competition {
                    builder.competition(competition);
                }

                if let Some(amount) = min_collateral_amount {
                    builder.min_output_amount(
                        token_amount(amount, None, token_map, &market, is_collateral_token_long)?
                            .into(),
                    );
                }
                if let Some(price) = acceptable_price {
                    builder.acceptable_price(unit_price(price, token_map, &market)?);
                }

                let (rpc, order) = builder.swap_path(swap.clone()).build_with_address().await?;

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
                    tracing::info!("created a limit increase order {order} at tx {signature}");

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
                min_output,
                acceptable_price,
            } => {
                let is_collateral_token_long = collateral_side.is_long();
                let market = if token_map.is_some() {
                    let market_address = client.find_market_address(store, market_token);
                    let market = client.market(&market_address).await?;
                    Some(market)
                } else {
                    None
                };
                let collateral_withdrawal_amount = if collateral_withdrawal_amount.is_zero() {
                    0
                } else {
                    token_amount(
                        collateral_withdrawal_amount,
                        final_output_token.as_ref(),
                        token_map.as_ref().expect("must exist"),
                        market.as_ref().expect("must exist"),
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

                if let Some(value) = min_output {
                    builder.min_output_amount(value.to_u128()?);
                }
                if let Some(price) = acceptable_price {
                    builder.acceptable_price(unit_price(
                        price,
                        token_map.as_ref().expect("must exist"),
                        market.as_ref().expect("must exist"),
                    )?);
                }

                let (rpc, order) = builder
                    .decrease_position_swap_type(Some(
                        DecreasePositionSwapType::PnlTokenToCollateralToken,
                    ))
                    .build_with_address()
                    .await?;

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
            Command::LimitDecrease {
                market_token,
                collateral_side,
                collateral_withdrawal_amount,
                side,
                price,
                size,
                final_output_token,
                swap,
                wait,
                competition,
                min_output,
                acceptable_price,
                valid_from_ts,
            }
            | Command::StopLoss {
                market_token,
                collateral_side,
                collateral_withdrawal_amount,
                side,
                price,
                size,
                final_output_token,
                swap,
                wait,
                competition,
                min_output,
                acceptable_price,
                valid_from_ts,
            } => {
                let market_address = client.find_market_address(store, market_token);
                let market = client.market(&market_address).await?;
                let token_map = token_map.as_ref().expect("must exist");
                let price = unit_price(price, token_map, &market)?;
                let is_collateral_token_long = collateral_side.is_long();
                let collateral_withdrawal_amount = token_amount(
                    collateral_withdrawal_amount,
                    final_output_token.as_ref(),
                    token_map,
                    &market,
                    is_collateral_token_long,
                )?;
                let mut builder = match &self.command {
                    Command::LimitDecrease { .. } => client.limit_decrease(
                        store,
                        market_token,
                        side.is_long(),
                        size.to_u128()?,
                        price,
                        is_collateral_token_long,
                        collateral_withdrawal_amount,
                    ),
                    Command::StopLoss { .. } => client.stop_loss(
                        store,
                        market_token,
                        side.is_long(),
                        size.to_u128()?,
                        price,
                        is_collateral_token_long,
                        collateral_withdrawal_amount,
                    ),
                    _ => unreachable!(),
                };
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(token) = final_output_token {
                    builder.final_output_token(token);
                }
                if let Some(competition) = competition {
                    builder.competition(competition);
                }
                if let Some(value) = min_output {
                    builder.min_output_amount(value.to_u128()?);
                }
                if let Some(price) = acceptable_price {
                    builder.acceptable_price(unit_price(price, token_map, &market)?);
                }
                if let Some(ts) = valid_from_ts {
                    builder.valid_from_ts(to_unix_timestamp(ts)?);
                }

                let (rpc, order) = builder
                    .swap_path(swap.clone())
                    .decrease_position_swap_type(Some(
                        DecreasePositionSwapType::PnlTokenToCollateralToken,
                    ))
                    .build_with_address()
                    .await?;
                println!("Order: {order}");

                let tx = if *wait {
                    ctx.require_not_serialize_only_mode()?;
                    ctx.require_not_ix_buffer_mode()?;

                    let signature = rpc.send_without_preflight().await?;
                    tracing::info!("created a limit decrease order {order} at tx {signature}");

                    wait_for_order(client, &order).await?;
                    return Ok(());
                } else {
                    rpc
                };
                tx.into_bundle_with_options(options)?
            }
            Command::UpdateOrder {
                address,
                price,
                acceptable_price,
                min_output,
                size,
                valid_from_ts,
            } => {
                let order = client.order(address).await?;
                let kind = order.params.kind()?;
                let market = if token_map.is_some() {
                    let market_address = client.find_market_address(store, &order.market_token);
                    Some(client.market(&market_address).await?)
                } else {
                    None
                };
                let min_output = match kind {
                    OrderKind::LimitDecrease | OrderKind::StopLossDecrease => min_output
                        .as_ref()
                        .map(|value| value.to_u128(MARKET_DECIMALS))
                        .transpose()?,
                    OrderKind::LimitIncrease => {
                        if let Some(amount) = min_output {
                            if token_map.is_none() {
                                token_map = Some(client.authorized_token_map(store).await?);
                            }
                            Some(
                                get_token_amount_with_token_map(
                                    amount,
                                    &order.params.collateral_token,
                                    token_map.as_ref().expect("must exist"),
                                )?
                                .into(),
                            )
                        } else {
                            None
                        }
                    }
                    OrderKind::LimitSwap => {
                        eyre::bail!(
                            "cannot update swap order with this command, use `update-swap` instead"
                        );
                    }
                    kind => {
                        eyre::bail!("{:?} is not updatable", kind);
                    }
                };
                let params = UpdateOrderParams {
                    size_delta_value: size.as_ref().map(|s| s.to_u128()).transpose()?,
                    acceptable_price: acceptable_price
                        .as_ref()
                        .map(|price| {
                            unit_price(
                                price,
                                token_map.as_ref().expect("must exist"),
                                market.as_ref().expect("must exist"),
                            )
                        })
                        .transpose()?,
                    trigger_price: price
                        .as_ref()
                        .map(|price| {
                            unit_price(
                                price,
                                token_map.as_ref().expect("must exist"),
                                market.as_ref().expect("must exist"),
                            )
                        })
                        .transpose()?,
                    min_output,
                    valid_from_ts: valid_from_ts.as_ref().map(to_unix_timestamp).transpose()?,
                };
                client
                    .update_order(store, &order.market_token, address, params, None)
                    .await?
                    .into_bundle_with_options(options)?
            }
            Command::MarketSwap {
                market_token,
                output_side,
                initial_swap_in_token,
                initial_swap_in_token_account,
                initial_swap_in_token_amount,
                swap,
                min_output_amount,
            } => {
                let token_map = token_map.as_ref().expect("must exist");
                let initial_swap_in_token_amount = get_token_amount_with_token_map(
                    initial_swap_in_token_amount,
                    initial_swap_in_token,
                    token_map,
                )?;
                let is_output_token_long = output_side.is_long();
                if let Some(c) = collector.as_mut() {
                    c.add_with_token(
                        initial_swap_in_token_amount,
                        owner,
                        initial_swap_in_token,
                        initial_swap_in_token_account.as_ref(),
                    )?;
                }
                let mut builder = client.market_swap(
                    store,
                    market_token,
                    is_output_token_long,
                    initial_swap_in_token,
                    initial_swap_in_token_amount,
                    swap.iter().chain(Some(market_token)),
                );
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(account) = initial_swap_in_token_account {
                    builder.initial_collateral_token(initial_swap_in_token, Some(account));
                }

                if let Some(amount) = min_output_amount {
                    let market_address = client.find_market_address(store, market_token);
                    let market = client.market(&market_address).await?;
                    builder.min_output_amount(
                        token_amount(amount, None, token_map, &market, is_output_token_long)?
                            .into(),
                    );
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

                rpc.into_bundle_with_options(options)?
            }
            Command::LimitSwap {
                market_token,
                output_side,
                price,
                initial_swap_in_token,
                initial_swap_in_token_account,
                initial_swap_in_token_amount,
                swap,
                valid_from_ts,
            } => {
                let token_map = token_map.as_ref().expect("must exist");
                let market = client
                    .market(&client.find_market_address(store, market_token))
                    .await?;
                let market_meta = MarketMeta::from(market.meta);
                let token_out = market_meta.pnl_token(output_side.is_long());
                let initial_swap_in_token_amount = get_token_amount_with_token_map(
                    initial_swap_in_token_amount,
                    initial_swap_in_token,
                    token_map,
                )?;
                if let Some(c) = collector.as_mut() {
                    c.add_with_token(
                        initial_swap_in_token_amount,
                        owner,
                        initial_swap_in_token,
                        initial_swap_in_token_account.as_ref(),
                    )?;
                }
                let min_output_amount = price_to_min_output_amount(
                    token_map,
                    initial_swap_in_token,
                    initial_swap_in_token_amount,
                    &token_out,
                    *price,
                )
                .ok_or_eyre("invalid price")?;
                let mut builder = client.limit_swap(
                    store,
                    market_token,
                    output_side.is_long(),
                    min_output_amount,
                    initial_swap_in_token,
                    initial_swap_in_token_amount,
                    swap.iter().chain(Some(market_token)),
                );
                if let Some(nonce) = nonce {
                    builder.nonce(nonce);
                }
                if let Some(account) = initial_swap_in_token_account {
                    builder.initial_collateral_token(initial_swap_in_token, Some(account));
                }
                if let Some(ts) = valid_from_ts {
                    builder.valid_from_ts(to_unix_timestamp(ts)?);
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

                rpc.into_bundle_with_options(options)?
            }
            Command::UpdateSwap {
                address,
                price,
                valid_from_ts,
            } => {
                let order = client.order(address).await?;
                if !matches!(order.params.kind()?, OrderKind::LimitSwap) {
                    eyre::bail!("the given order is not a limit-swap order");
                }
                let token_map = client
                    .token_map(
                        &client
                            .authorized_token_map_address(store)
                            .await?
                            .ok_or_eyre("token map is not set")?,
                    )
                    .await?;
                let min_output_amount = price
                    .as_ref()
                    .map(|price| {
                        price_to_min_output_amount(
                            &token_map,
                            &order
                                .tokens
                                .initial_collateral
                                .token()
                                .ok_or_eyre("missing swap in token")?,
                            order.params.initial_collateral_delta_amount,
                            &order
                                .tokens
                                .final_output_token
                                .token()
                                .ok_or_eyre("missing swap out token")?,
                            *price,
                        )
                        .ok_or_eyre("invalid price")
                    })
                    .transpose()?;
                let params = UpdateOrderParams {
                    size_delta_value: None,
                    acceptable_price: None,
                    trigger_price: None,
                    min_output: min_output_amount.map(Into::into),
                    valid_from_ts: valid_from_ts.as_ref().map(to_unix_timestamp).transpose()?,
                };

                client
                    .update_order(store, &order.market_token, address, params, None)
                    .await?
                    .into_bundle_with_options(options)?
            }
            Command::CancelOrderIfNoPosition { order, keep } => {
                let cancel = client
                    .cancel_order_if_no_position(store, order, None)
                    .await?;
                let rpc = if *keep {
                    cancel
                } else {
                    let close = client.close_order(order)?.build().await?;
                    cancel.merge(close)
                };

                rpc.into_bundle_with_options(options)?
            }
            Command::Glv { glv_token, command } => {
                let glv_token = glv_token.address(client, store);

                let txn = match command {
                    GlvCommand::CloseDeposit { glv_deposit } => {
                        client.close_glv_deposit(glv_deposit).build().await?
                    }
                    GlvCommand::CloseShift { glv_shift } => {
                        client.close_glv_shift(glv_shift).build().await?
                    }
                    GlvCommand::CloseWithdrawal { glv_withdrawal } => {
                        client.close_glv_withdrawal(glv_withdrawal).build().await?
                    }
                    GlvCommand::CreateDeposit {
                        market_token,
                        receiver,
                        first_deposit,
                        extra_execution_fee,
                        min_amount,
                        min_market_token_amount,
                        long_token,
                        short_token,
                        market_token_account,
                        long_token_account,
                        short_token_account,
                        market_token_amount,
                        long_token_amount,
                        short_token_amount,
                        long_swap,
                        short_swap,
                    } => {
                        let market = if token_map.is_some() {
                            Some(client.market_by_token(store, market_token).await?)
                        } else {
                            None
                        };
                        let mut builder =
                            client.create_glv_deposit(store, &glv_token, market_token);
                        if let Some(nonce) = nonce {
                            builder.nonce(nonce);
                        }
                        if !market_token_amount.is_zero() {
                            builder.market_token_deposit(
                                market_token_amount.to_u64()?,
                                market_token_account.as_ref(),
                            );
                        }
                        if !long_token_amount.is_zero() {
                            let market = market.as_ref().expect("must exist");
                            let long_token_amount = token_amount(
                                long_token_amount,
                                long_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                market,
                                true,
                            )?;
                            builder.long_token_deposit(
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
                                    market,
                                    true,
                                )?;
                            }
                        }
                        if !short_token_amount.is_zero() {
                            let market = market.as_ref().expect("must exist");
                            let short_token_amount = token_amount(
                                short_token_amount,
                                short_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                market,
                                false,
                            )?;
                            builder.short_token_deposit(
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
                                    market,
                                    false,
                                )?;
                            }
                        }
                        let (rpc, deposit) = builder
                            .max_execution_fee(
                                extra_execution_fee.to_u64()? + glv_deposit::MIN_EXECUTION_LAMPORTS,
                            )
                            .min_glv_token_amount(min_amount.to_u64()?)
                            .min_market_token_amount(min_market_token_amount.to_u64()?)
                            .long_token_swap_path(long_swap.clone())
                            .short_token_swap_path(short_swap.clone())
                            .receiver(if *first_deposit {
                                Some(client.find_first_deposit_owner_address())
                            } else {
                                *receiver
                            })
                            .build_with_address()
                            .await?;
                        println!("GLV deposit: {deposit}");
                        rpc
                    }
                    GlvCommand::CreateWithdrawal {
                        market_token,
                        receiver,
                        extra_execution_fee,
                        amount,
                        final_long_token,
                        final_short_token,
                        glv_token_account,
                        min_final_long_token_amount,
                        min_final_short_token_amount,
                        long_swap,
                        short_swap,
                    } => {
                        let market = if token_map.is_some() {
                            Some(client.market_by_token(store, market_token).await?)
                        } else {
                            None
                        };
                        let mut builder = client.create_glv_withdrawal(
                            store,
                            &glv_token,
                            market_token,
                            amount.to_u64()?,
                        );
                        if let Some(nonce) = nonce {
                            builder.nonce(nonce);
                        }
                        if let Some(account) = glv_token_account {
                            builder.glv_token_source(account);
                        }
                        let min_final_long_token_amount = if min_final_long_token_amount.is_zero() {
                            0
                        } else {
                            token_amount(
                                min_final_long_token_amount,
                                final_long_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                market.as_ref().expect("must exist"),
                                true,
                            )?
                        };
                        let min_final_short_token_amount = if min_final_short_token_amount.is_zero()
                        {
                            0
                        } else {
                            token_amount(
                                min_final_short_token_amount,
                                final_short_token.as_ref(),
                                token_map.as_ref().expect("must exist"),
                                market.as_ref().expect("must exist"),
                                false,
                            )?
                        };
                        builder
                            .final_long_token(
                                final_long_token.as_ref(),
                                min_final_long_token_amount,
                                long_swap.clone(),
                            )
                            .final_short_token(
                                final_short_token.as_ref(),
                                min_final_short_token_amount,
                                short_swap.clone(),
                            );
                        let (rpc, withdrawal) = builder
                            .max_execution_fee(
                                extra_execution_fee.to_u64()?
                                    + glv_withdrawal::MIN_EXECUTION_LAMPORTS,
                            )
                            .receiver(*receiver)
                            .build_with_address()
                            .await?;
                        println!("GLV withdrawal: {withdrawal}");
                        rpc
                    }
                    GlvCommand::CreateShift {
                        from,
                        to,
                        amount,
                        min_output_amount,
                        extra_execution_fee,
                    } => {
                        let mut builder =
                            client.create_glv_shift(store, &glv_token, from, to, amount.to_u64()?);
                        if let Some(nonce) = nonce {
                            builder.nonce(nonce);
                        }
                        builder
                            .execution_fee(
                                extra_execution_fee.to_u64()? + glv_shift::MIN_EXECUTION_LAMPORTS,
                            )
                            .min_to_market_token_amount(min_output_amount.to_u64()?);

                        let (rpc, shift) = builder.build_with_address()?;

                        println!("GLV shift: {shift}");

                        rpc
                    }
                };

                txn.into_bundle_with_options(options)?
            }
            #[cfg(feature = "execute")]
            Command::Execute {
                args,
                address,
                skip_close,
                throw_error_on_failure,
            } => {
                use gmsol_sdk::decode::gmsol::programs::GMSOLAccountData;

                ctx.require_not_serialize_only_mode()?;
                ctx.require_not_ix_buffer_mode()?;

                let decoded = client
                    .decode_account_with_config(address, Default::default())
                    .await?
                    .into_value()
                    .ok_or_eyre("account not found")?;
                let executor = args.build(client).await?;
                let oracle = ctx.config().oracle()?;
                match decoded {
                    GMSOLAccountData::Deposit(_) => {
                        let mut builder = client.execute_deposit(
                            store,
                            oracle,
                            address,
                            !*throw_error_on_failure,
                        );
                        builder.close(!*skip_close);
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::Withdrawal(_) => {
                        let mut builder = client.execute_withdrawal(
                            store,
                            oracle,
                            address,
                            !*throw_error_on_failure,
                        );
                        builder.close(!*skip_close);
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::Shift(_) => {
                        let mut builder =
                            client.execute_shift(oracle, address, !*throw_error_on_failure);
                        builder.close(!*skip_close);
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::GlvDeposit(_) => {
                        let mut builder =
                            client.execute_glv_deposit(oracle, address, !*throw_error_on_failure);
                        builder.close(!*skip_close);
                        for alt in ctx.config().alts() {
                            let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::GlvWithdrawal(_) => {
                        let mut builder = client.execute_glv_withdrawal(
                            oracle,
                            address,
                            !*throw_error_on_failure,
                        );
                        builder.close(!*skip_close);
                        for alt in ctx.config().alts() {
                            let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::GlvShift(_) => {
                        let mut builder =
                            client.execute_glv_shift(oracle, address, !*throw_error_on_failure);
                        builder.close(!*skip_close);
                        for alt in ctx.config().alts() {
                            let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        executor.execute(builder, options).await?;
                    }
                    GMSOLAccountData::Order(_) => {
                        let mut builder = client.execute_order(
                            store,
                            oracle,
                            address,
                            !*throw_error_on_failure,
                        )?;
                        for alt in ctx.config().alts() {
                            let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                            builder.add_alt(alt);
                        }
                        builder.close(!*skip_close);
                        executor.execute(builder, options).await?;
                    }
                    _ => {
                        eyre::bail!("unsupported");
                    }
                }
                return Ok(());
            }
            #[cfg(feature = "execute")]
            Command::UpdateAdl {
                args,
                market_token,
                side,
            } => {
                ctx.require_not_serialize_only_mode()?;
                ctx.require_not_ix_buffer_mode()?;

                let executor = args.build(client).await?;
                let oracle = ctx.config().oracle()?;
                let (for_long, for_short) = match side {
                    Some(Side::Long) => (true, false),
                    Some(Side::Short) => (false, true),
                    None => (true, true),
                };
                let builder =
                    client.update_adl(store, oracle, market_token, for_long, for_short)?;
                executor.execute(builder, options).await?;
                return Ok(());
            }
            #[cfg(feature = "execute")]
            Command::Adl {
                args,
                position,
                size,
                close_all,
            } => {
                ctx.require_not_serialize_only_mode()?;
                ctx.require_not_ix_buffer_mode()?;

                let executor = args.build(client).await?;
                let oracle = ctx.config().oracle()?;
                let size = match size {
                    Some(size) => *size,
                    None => {
                        debug_assert!(*close_all);
                        let position = client.position(position).await?;
                        position.state.size_in_usd
                    }
                };
                let mut builder = client.auto_deleverage(oracle, position, size)?;
                for alt in ctx.config().alts() {
                    let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                executor.execute(builder, options).await?;
                return Ok(());
            }
            #[cfg(feature = "execute")]
            Command::Liquidate { args, position } => {
                ctx.require_not_serialize_only_mode()?;
                ctx.require_not_ix_buffer_mode()?;

                let executor = args.build(client).await?;
                let oracle = ctx.config().oracle()?;
                let mut builder = client.liquidate(oracle, position)?;
                for alt in ctx.config().alts() {
                    let alt = client.alt(alt).await?.ok_or(gmsol_sdk::Error::NotFound)?;
                    builder.add_alt(alt);
                }
                executor.execute(builder, options).await?;
                return Ok(());
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
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
    fn add_with_token(
        &mut self,
        amount: u64,
        owner: &Pubkey,
        token: &Pubkey,
        token_account: Option<&Pubkey>,
    ) -> eyre::Result<()> {
        use anchor_spl::{
            associated_token::get_associated_token_address, token::spl_token::native_mint::ID,
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

    fn add(
        &mut self,
        amount: u64,
        owner: &Pubkey,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
        market: &Market,
        is_long: bool,
    ) -> eyre::Result<()> {
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

        self.add_with_token(amount, owner, token, token_account)
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

fn to_unix_timestamp(ts: &humantime::Timestamp) -> eyre::Result<i64> {
    use std::time::SystemTime;

    Ok(ts
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}
