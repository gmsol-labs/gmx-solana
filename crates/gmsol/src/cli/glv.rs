use crate::{utils::ToggleValue, GMSOLClient, TimelockCtx};
use gmsol::{
    store::glv::GlvOps,
    types::{common::action::Action, glv::GlvMarketFlag, GlvDeposit},
};
use solana_sdk::pubkey::Pubkey;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(flatten)]
    glv_token: GlvToken,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Inititalize a GLV.
    Init {
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
    /// Toggle GLV market flag.
    Toggle {
        market_token: Pubkey,
        #[arg(long)]
        flag: GlvMarketFlag,
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Update.
    Update {
        market_token: Pubkey,
        #[command(flatten)]
        config: Config,
    },
    /// Create GLV deposit.
    Deposit {
        /// The address of the market token of the Market to deposit into.
        market_token: Pubkey,
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = 0)]
        extra_execution_fee: u64,
        /// Minimum amount of GLV tokens to mint.
        #[arg(long, default_value_t = 0)]
        min_amount: u64,
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
        #[arg(long, default_value_t = 0)]
        market_token_amount: u64,
        #[arg(long, default_value_t = 0)]
        long_token_amount: u64,
        /// The initial short token amount.
        #[arg(long, default_value_t = 0)]
        short_token_amount: u64,
        /// Swap paths for long token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        long_swap: Vec<Pubkey>,
        /// Swap paths for short token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        short_swap: Vec<Pubkey>,
    },
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct GlvToken {
    /// GLV token address.
    #[arg(long)]
    glv_token: Option<Pubkey>,
    /// Index.
    #[arg(long)]
    index: Option<u8>,
}

#[derive(clap::Args)]
#[group(required = true, multiple = true)]
struct Config {
    #[arg(long)]
    max_amount: Option<u64>,
    #[arg(long)]
    max_value: Option<u128>,
}

impl GlvToken {
    fn address(&self, client: &GMSOLClient, store: &Pubkey) -> Pubkey {
        match (self.glv_token, self.index) {
            (Some(address), _) => address,
            (None, Some(index)) => client.find_glv_token_address(store, index),
            (None, None) => unreachable!(),
        }
    }
}

impl Args {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        timelock: Option<TimelockCtx<'_>>,
        serialize_only: bool,
        skip_preflight: bool,
    ) -> gmsol::Result<()> {
        let selected = &self.glv_token;
        let rpc = match &self.command {
            Command::Init { market_tokens } => {
                let Some(index) = selected.index else {
                    return Err(gmsol::Error::invalid_argument(
                        "must provide index to init GLV",
                    ));
                };
                let (rpc, glv_token) =
                    client.initialize_glv(store, index, market_tokens.iter().copied())?;
                if glv_token != selected.address(client, store) {
                    return Err(gmsol::Error::invalid_argument(
                        "the GLV token to be initialized is not the selected one",
                    ));
                }
                println!("{glv_token}");
                rpc
            }
            Command::Toggle {
                market_token,
                flag,
                toggle,
            } => client.toggle_glv_market_flag(
                store,
                &selected.address(client, store),
                market_token,
                *flag,
                toggle.is_enable(),
            ),
            Command::Update {
                market_token,
                config,
            } => client.update_glv_market_config(
                store,
                &selected.address(client, store),
                market_token,
                config.max_amount,
                config.max_value,
            ),
            Command::Deposit {
                market_token,
                receiver,
                extra_execution_fee,
                min_amount,
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
                let glv_token = selected.address(client, store);
                let mut builder = client.create_glv_deposit(store, &glv_token, market_token);
                if *market_token_amount != 0 {
                    builder
                        .market_token_deposit(*market_token_amount, market_token_account.as_ref());
                }
                if *long_token_amount != 0 {
                    builder.long_token_deposit(
                        *long_token_amount,
                        long_token.as_ref(),
                        long_token_account.as_ref(),
                    );
                }
                if *short_token_amount != 0 {
                    builder.short_token_deposit(
                        *short_token_amount,
                        short_token.as_ref(),
                        short_token_account.as_ref(),
                    );
                }
                let (rpc, deposit) = builder
                    .max_execution_fee(*extra_execution_fee + GlvDeposit::MIN_EXECUTION_LAMPORTS)
                    .min_glv_token_amount(*min_amount)
                    .long_token_swap_path(long_swap.clone())
                    .short_token_swap_path(short_swap.clone())
                    .receiver(*receiver)
                    .build_with_address()
                    .await?;
                println!("{deposit}");
                rpc
            }
        };

        crate::utils::send_or_serialize_rpc(
            store,
            rpc,
            timelock,
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
