use std::path::PathBuf;

use crate::{
    ser::SerdeFactor,
    utils::{toml_from_file, ToggleValue},
    GMSOLClient, InstructionBufferCtx,
};
use gmsol::{
    store::glv::GlvOps,
    types::{
        common::action::Action,
        glv::{GlvMarketFlag, UpdateGlvParams},
        GlvDeposit, GlvShift, GlvWithdrawal,
    },
    utils::instruction::InstructionSerialization,
};
use gmsol_solana_utils::bundle_builder::BundleOptions;
use indexmap::IndexMap;
use serde_with::serde_as;
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
    /// Update Config.
    Update(UpdateGlvArgs),
    /// Toggle GLV market flag.
    ToggleMarketFlag {
        market_token: Pubkey,
        #[arg(long)]
        flag: GlvMarketFlag,
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Update Market Config.
    UpdateMarket {
        market_token: Pubkey,
        #[command(flatten)]
        config: Config,
    },
    /// Insert Market.
    InsertMarket {
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
    /// Remove Market.
    RemoveMarket {
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
    /// Create GLV deposit.
    Deposit {
        /// The address of the market token of the GLV Market to deposit into.
        market_token: Pubkey,
        /// Nonce.
        #[arg(long)]
        nonce: Option<Pubkey>,
        #[arg(long, group = "deposit-receiver")]
        receiver: Option<Pubkey>,
        #[arg(long, group = "deposit-receiver", requires = "min_amount")]
        first_deposit: bool,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = 0)]
        extra_execution_fee: u64,
        /// Minimum amount of GLV tokens to mint.
        #[arg(long, default_value_t = 0)]
        min_amount: u64,
        /// Minimum amount of market tokens to mint.
        #[arg(long, default_value_t = 0)]
        min_market_token_amount: u64,
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
    /// Create a GLV withdrawal.
    Withdraw {
        /// The address of the market token of the GLV Market to withdraw from.
        market_token: Pubkey,
        /// Nonce.
        #[arg(long)]
        nonce: Option<Pubkey>,
        #[arg(long)]
        receiver: Option<Pubkey>,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = 0)]
        extra_execution_fee: u64,
        /// The amount of GLV tokens to burn.
        #[arg(long)]
        amount: u64,
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
        #[arg(long, default_value_t = 0)]
        min_final_long_token_amount: u64,
        /// Minimal amount of final short tokens to withdraw.
        #[arg(long, default_value_t = 0)]
        min_final_short_token_amount: u64,
        /// Swap paths for long token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        long_swap: Vec<Pubkey>,
        /// Swap paths for short token.
        #[arg(long, short, action = clap::ArgAction::Append)]
        short_swap: Vec<Pubkey>,
    },
    /// Create a GLV shift.
    Shift {
        /// Nonce.
        #[arg(long)]
        nonce: Option<Pubkey>,
        /// From market token.
        #[arg(long, value_name = "FROM_MARKET_TOKEN")]
        from: Pubkey,
        /// To market token.
        #[arg(long, value_name = "TO_MARKET_TOKEN")]
        to: Pubkey,
        /// Amount.
        #[arg(long)]
        amount: u64,
        /// Min output amount.
        #[arg(long, default_value_t = 0)]
        min_output_amount: u64,
        /// Extra execution fee allowed to use.
        #[arg(long, short, default_value_t = 0)]
        extra_execution_fee: u64,
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
    index: Option<u16>,
}

#[derive(clap::Args)]
#[group(required = true, multiple = true)]
struct UpdateGlvArgs {
    /// Path to the update file (TOML).
    #[arg(long, short)]
    file: Option<PathBuf>,
    /// Minimum amount for the first GLV deposit.
    #[arg(long)]
    min_tokens_for_first_deposit: Option<u64>,
    /// Minimum shift interval seconds.
    #[arg(long)]
    shift_min_interval_secs: Option<u32>,
    /// Maximum price impact factor after shift.
    #[arg(long)]
    shift_max_price_impact_factor: Option<u128>,
    /// Minimum shift value.
    #[arg(long)]
    shift_min_value: Option<u128>,
}

impl<'a> From<&'a UpdateGlvArgs> for UpdateGlvParams {
    fn from(args: &'a UpdateGlvArgs) -> Self {
        Self {
            min_tokens_for_first_deposit: args.min_tokens_for_first_deposit,
            shift_min_interval_secs: args.shift_min_interval_secs,
            shift_max_price_impact_factor: args.shift_max_price_impact_factor,
            shift_min_value: args.shift_min_value,
        }
    }
}

#[derive(clap::Args, Debug, serde::Serialize, serde::Deserialize)]
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
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        ctx: Option<InstructionBufferCtx<'_>>,
        serialize_only: Option<InstructionSerialization>,
        skip_preflight: bool,
        priority_lamports: u64,
        max_transaction_size: Option<usize>,
    ) -> gmsol::Result<()> {
        let selected = &self.glv_token;
        let mut rpc = match &self.command {
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
            Command::Update(args) => {
                let glv_token = selected.address(client, store);
                match &args.file {
                    Some(file) => {
                        let UpdateGlv { glv, market } = toml_from_file(file)?;

                        let mut bundle = client.bundle_with_options(BundleOptions {
                            max_packet_size: max_transaction_size,
                            ..Default::default()
                        });

                        let params: UpdateGlvParams = glv.try_into()?;
                        if !params.is_empty() {
                            bundle.push(client.update_glv_config(store, &glv_token, params))?;
                        }

                        for (market_token, MarketConfigWithFlag { config, flag }) in market {
                            bundle.push(client.update_glv_market_config(
                                store,
                                &glv_token,
                                &market_token,
                                config.max_amount()?,
                                config.max_value(),
                            ))?;

                            for (flag, enable) in flag {
                                bundle.push(client.toggle_glv_market_flag(
                                    store,
                                    &glv_token,
                                    &market_token,
                                    flag,
                                    enable,
                                ))?;
                            }
                        }

                        return crate::utils::send_or_serialize_bundle(
                            store,
                            bundle,
                            ctx,
                            serialize_only,
                            skip_preflight,
                            |signatures, err| {
                                tracing::info!("{signatures:#?}");
                                match err {
                                    None => Ok(()),
                                    Some(err) => Err(err),
                                }
                            },
                        )
                        .await;
                    }
                    None => client.update_glv_config(store, &glv_token, args.into()),
                }
            }
            Command::ToggleMarketFlag {
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
            Command::UpdateMarket {
                market_token,
                config,
            } => client.update_glv_market_config(
                store,
                &selected.address(client, store),
                market_token,
                config.max_amount,
                config.max_value,
            ),
            Command::InsertMarket { market_tokens } => {
                let mut bundle = client.bundle_with_options(BundleOptions {
                    max_packet_size: max_transaction_size,
                    ..Default::default()
                });
                let glv_token = selected.address(client, store);
                for market_token in market_tokens {
                    bundle.push(client.insert_glv_market(store, &glv_token, market_token, None))?;
                }
                return crate::utils::send_or_serialize_bundle_with_default_callback(
                    store,
                    bundle,
                    ctx,
                    serialize_only,
                    skip_preflight,
                )
                .await;
            }
            Command::RemoveMarket { market_tokens } => {
                let mut bundle = client.bundle_with_options(BundleOptions {
                    max_packet_size: max_transaction_size,
                    ..Default::default()
                });
                let glv_token = selected.address(client, store);

                for market_token in market_tokens {
                    bundle.push(client.remove_glv_market(store, &glv_token, market_token, None))?;
                }
                return crate::utils::send_or_serialize_bundle_with_default_callback(
                    store,
                    bundle,
                    ctx,
                    serialize_only,
                    skip_preflight,
                )
                .await;
            }
            Command::Deposit {
                market_token,
                nonce,
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
                let glv_token = selected.address(client, store);
                let mut builder = client.create_glv_deposit(store, &glv_token, market_token);
                if let Some(nonce) = nonce.map(|p| p.to_bytes()) {
                    builder.nonce(nonce);
                }
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
                    .min_market_token_amount(*min_market_token_amount)
                    .long_token_swap_path(long_swap.clone())
                    .short_token_swap_path(short_swap.clone())
                    .receiver(if *first_deposit {
                        Some(GlvDeposit::first_deposit_receiver())
                    } else {
                        *receiver
                    })
                    .build_with_address()
                    .await?;
                println!("{deposit}");
                rpc
            }
            Command::Withdraw {
                market_token,
                nonce,
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
                let mut builder = client.create_glv_withdrawal(
                    store,
                    &selected.address(client, store),
                    market_token,
                    *amount,
                );
                if let Some(nonce) = nonce.map(|p| p.to_bytes()) {
                    builder.nonce(nonce);
                }
                if let Some(account) = glv_token_account {
                    builder.glv_token_source(account);
                }
                builder
                    .final_long_token(
                        final_long_token.as_ref(),
                        *min_final_long_token_amount,
                        long_swap.clone(),
                    )
                    .final_short_token(
                        final_short_token.as_ref(),
                        *min_final_short_token_amount,
                        short_swap.clone(),
                    );
                let (rpc, withdrawal) = builder
                    .max_execution_fee(*extra_execution_fee + GlvWithdrawal::MIN_EXECUTION_LAMPORTS)
                    .receiver(*receiver)
                    .build_with_address()
                    .await?;
                println!("{withdrawal}");
                rpc
            }
            Command::Shift {
                nonce,
                from,
                to,
                amount,
                min_output_amount,
                extra_execution_fee,
            } => {
                let mut builder = client.create_glv_shift(
                    store,
                    &selected.address(client, store),
                    from,
                    to,
                    *amount,
                );
                if let Some(nonce) = nonce.map(|p| p.to_bytes()) {
                    builder.nonce(nonce);
                }
                builder
                    .execution_fee(extra_execution_fee + GlvShift::MIN_EXECUTION_LAMPORTS)
                    .min_to_market_token_amount(*min_output_amount);

                let (rpc, shift) = builder.build_with_address()?;

                println!("{shift}");

                rpc
            }
        };

        rpc.compute_budget_mut()
            .set_min_priority_lamports(Some(priority_lamports));

        crate::utils::send_or_serialize_transaction(
            store,
            rpc,
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

#[serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UpdateGlv {
    #[serde(flatten)]
    glv: GlvConfig,
    #[serde(flatten)]
    #[serde_as(as = "IndexMap<serde_with::DisplayFromStr, _>")]
    market: IndexMap<Pubkey, MarketConfigWithFlag>,
}

#[serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GlvConfig {
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    min_tokens_for_first_deposit: Option<SerdeFactor>,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    shift_min_interval: Option<humantime::Duration>,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    shift_max_price_impact_factor: Option<SerdeFactor>,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    shift_min_value: Option<SerdeFactor>,
}

impl TryFrom<GlvConfig> for UpdateGlvParams {
    type Error = gmsol::Error;

    fn try_from(config: GlvConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            min_tokens_for_first_deposit: config
                .min_tokens_for_first_deposit
                .map(|f| f.0.try_into().map_err(gmsol::Error::unknown))
                .transpose()?,
            shift_min_interval_secs: config
                .shift_min_interval
                .map(|d| d.as_secs().try_into().map_err(gmsol::Error::unknown))
                .transpose()?,
            shift_max_price_impact_factor: config.shift_max_price_impact_factor.map(|f| f.0),
            shift_min_value: config.shift_min_value.map(|f| f.0),
        })
    }
}

#[serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MarketConfig {
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    max_amount: Option<SerdeFactor>,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    max_value: Option<SerdeFactor>,
}

impl MarketConfig {
    fn max_amount(&self) -> gmsol::Result<Option<u64>> {
        self.max_amount
            .as_ref()
            .map(|f| f.0.try_into().map_err(gmsol::Error::unknown))
            .transpose()
    }

    fn max_value(&self) -> Option<u128> {
        self.max_value.as_ref().map(|f| f.0)
    }
}

#[serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MarketConfigWithFlag {
    #[serde(flatten)]
    config: MarketConfig,
    #[serde(flatten)]
    #[serde_as(as = "IndexMap<serde_with::DisplayFromStr, _>")]
    flag: IndexMap<GlvMarketFlag, bool>,
}
