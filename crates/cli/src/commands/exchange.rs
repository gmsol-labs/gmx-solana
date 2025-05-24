use eyre::OptionExt;
use gmsol_sdk::{
    builders::NonceBytes,
    client::token_map::TokenMap,
    core::token_config::TokenMapAccess,
    ops::{exchange::deposit, ExchangeOps},
    programs::{anchor_lang::prelude::Pubkey, gmsol_store::accounts::Market},
};

use super::utils::{Amount, GmAmount, Lamport};

/// Commands for exchange functionalities.
#[derive(Debug, clap::Args)]
pub struct Exchange {
    /// Nonce for actions.
    #[arg(long)]
    nonce: Option<NonceBytes>,
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
}

impl super::Command for Exchange {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let nonce = self.nonce.map(|nonce| nonce.to_bytes());
        let store = ctx.store();
        let client = ctx.client()?;
        let token_map = client.authorized_token_map(store).await?;
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
                if !long_token_amount.0.is_zero() {
                    let long_token_amount = token_amount(
                        long_token_amount,
                        long_token.as_ref(),
                        &token_map,
                        &market,
                        true,
                    )?;
                    builder.long_token(
                        long_token_amount,
                        long_token.as_ref(),
                        long_token_account.as_ref(),
                    );
                }
                if !short_token_amount.0.is_zero() {
                    let short_token_amount = token_amount(
                        short_token_amount,
                        short_token.as_ref(),
                        &token_map,
                        &market,
                        false,
                    )?;
                    builder.short_token(
                        short_token_amount,
                        short_token.as_ref(),
                        short_token_account.as_ref(),
                    );
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
                builder.into_bundle_with_options(ctx.bundle_options())?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
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
