use std::{collections::BTreeMap, ops::Deref, path::PathBuf};

use eyre::OptionExt;
use gmsol_sdk::{
    core::glv::GlvMarketFlag,
    ops::GlvOps,
    programs::gmsol_store::types::UpdateGlvParams,
    serde::{serde_glv::SerdeGlv, StringPubkey},
    solana_utils::solana_sdk::{pubkey::Pubkey, signer::Signer},
    utils::{zero_copy::ZeroCopy, GmAmount, Value},
};
use indexmap::IndexMap;

use crate::config::DisplayOptions;

use super::utils::{toml_from_file, ToggleValue};

/// GLV management commands.
#[derive(Debug, clap::Args)]
pub struct Glv {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Display the current status of the selected GLV.
    Get {
        /// GLV token address.
        #[arg(group = "select-glv")]
        glv_token: Option<Pubkey>,
        /// Index.
        #[arg(long, group = "select-glv")]
        index: Option<u16>,
    },
    /// Inititalize a GLV.
    Init {
        #[command(flatten)]
        glv_token: GlvToken,
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
    /// Update Config.
    UpdateConfig {
        #[command(flatten)]
        glv_token: GlvToken,
        #[command(flatten)]
        args: UpdateGlvArgs,
    },
    /// Toggle GLV market flag.
    ToggleMarketFlag {
        #[command(flatten)]
        glv_token: GlvToken,
        market_token: Pubkey,
        #[arg(long)]
        flag: GlvMarketFlag,
        #[command(flatten)]
        toggle: ToggleValue,
    },
    /// Update Market Config.
    UpdateMarket {
        #[command(flatten)]
        glv_token: GlvToken,
        market_token: Pubkey,
        #[command(flatten)]
        config: MarketConfig,
    },
    /// Insert Market.
    InsertMarket {
        #[command(flatten)]
        glv_token: GlvToken,
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
    /// Remove Market.
    RemoveMarket {
        #[command(flatten)]
        glv_token: GlvToken,
        #[clap(required = true)]
        market_tokens: Vec<Pubkey>,
    },
}

impl super::Command for Glv {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::Get { glv_token, index } => {
                let output = ctx.config().output();
                let glv_token = match (glv_token, index) {
                    (Some(address), None) => Some(*address),
                    (None, Some(index)) => Some(client.find_glv_token_address(store, *index)),
                    (None, None) => None,
                    _ => unreachable!(),
                };
                match glv_token {
                    Some(glv_token) => {
                        let glv_address = client.find_glv_address(&glv_token);
                        let glv = client
                            .account::<ZeroCopy<gmsol_sdk::programs::gmsol_store::accounts::Glv>>(
                                &glv_address,
                            )
                            .await?
                            .ok_or_eyre("GLV not found")?;
                        let glv = SerdeGlv::from_glv(&glv.0)?;
                        println!(
                            "{}",
                            output.display_keyed_account(
                                &glv_address,
                                &glv,
                                DisplayOptions::table_projection([
                                    ("pubkey", "Address"),
                                    ("glv_token", "GLV Token"),
                                    ("shift_last_executed_at", "Shift Last Executed"),
                                    ("shift_min_interval_secs", "Shift Min Interval"),
                                    ("shift_min_value", "Shift Min Value"),
                                    (
                                        "min_tokens_for_first_deposit",
                                        "Min tokens for first deposit"
                                    ),
                                ])
                            )?
                        );
                        println!(
                            "{}",
                            output.display_keyed_accounts(
                                glv.markets,
                                DisplayOptions::table_projection([
                                    ("pubkey", "Market Token"),
                                    ("balance", "Vault Balance"),
                                    ("max_amount", "Max Amount"),
                                    ("max_value", "Max Value"),
                                    ("is_deposit_allowed", "Allow Deposit"),
                                ])
                            )?
                        );
                    }
                    None => {
                        let glvs = client.glvs(store).await?;
                        let glvs = glvs
                            .iter()
                            .filter(|(_, v)| {
                                client.find_glv_token_address(store, v.index) == v.glv_token
                            })
                            .map(|(k, v)| Ok((k, SerdeGlv::from_glv(v)?)))
                            .collect::<eyre::Result<BTreeMap<_, _>>>()?;
                        println!(
                            "{}",
                            output.display_keyed_accounts(
                                glvs,
                                DisplayOptions::table_projection([
                                    ("glv_token", "GLV token"),
                                    ("index", "Index"),
                                    ("long_token", "Long Token"),
                                    ("short_token", "Short Token"),
                                ])
                            )?
                        );
                    }
                }

                return Ok(());
            }
            Command::Init {
                glv_token: selected,
                market_tokens,
            } => {
                let Some(index) = selected.index else {
                    eyre::bail!("must provide --index to init GLV");
                };
                let (rpc, glv_token) =
                    client.initialize_glv(store, index, market_tokens.iter().copied())?;
                if glv_token != selected.address(client, store) {
                    eyre::bail!("the GLV token to be initialized is not the selected one");
                }
                println!("GLV Token: {glv_token}");
                rpc.into_bundle_with_options(options)?
            }
            Command::UpdateConfig { glv_token, args } => {
                let glv_token = glv_token.address(client, store);
                match &args.file {
                    Some(file) => {
                        let UpdateGlv { glv, market } = toml_from_file(file)?;

                        let mut bundle = client.bundle_with_options(options);

                        let params: UpdateGlvParams = glv.try_into()?;
                        if params.min_tokens_for_first_deposit.is_some()
                            || params.shift_max_price_impact_factor.is_some()
                            || params.shift_min_interval_secs.is_some()
                            || params.shift_min_value.is_some()
                        {
                            bundle.push(client.update_glv_config(store, &glv_token, params))?;
                        }

                        for (market_token, MarketConfigWithFlag { config, flag }) in market {
                            bundle.push(client.update_glv_market_config(
                                store,
                                &glv_token,
                                &market_token,
                                config.max_amount()?,
                                config.max_value()?,
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

                        bundle
                    }
                    None => client
                        .update_glv_config(store, &glv_token, args.try_into()?)
                        .into_bundle_with_options(options)?,
                }
            }
            Command::ToggleMarketFlag {
                glv_token: selected,
                market_token,
                flag,
                toggle,
            } => client
                .toggle_glv_market_flag(
                    store,
                    &selected.address(client, store),
                    market_token,
                    *flag,
                    toggle.is_enable(),
                )
                .into_bundle_with_options(options)?,
            Command::UpdateMarket {
                glv_token: selected,
                market_token,
                config,
            } => client
                .update_glv_market_config(
                    store,
                    &selected.address(client, store),
                    market_token,
                    config.max_amount()?,
                    config.max_value()?,
                )
                .into_bundle_with_options(options)?,
            Command::InsertMarket {
                glv_token: selected,
                market_tokens,
            } => {
                let mut bundle = client.bundle_with_options(options);
                let glv_token = selected.address(client, store);
                for market_token in market_tokens {
                    bundle.push(client.insert_glv_market(store, &glv_token, market_token, None))?;
                }
                bundle
            }
            Command::RemoveMarket {
                glv_token: selected,
                market_tokens,
            } => {
                let mut bundle = client.bundle_with_options(options);
                let glv_token = selected.address(client, store);

                for market_token in market_tokens {
                    bundle.push(client.remove_glv_market(store, &glv_token, market_token, None))?;
                }

                bundle
            }
        };

        client.send_or_serialize(bundle).await?;

        Ok(())
    }
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct GlvToken {
    /// GLV token address.
    #[arg(long)]
    glv_token: Option<Pubkey>,
    /// Index.
    #[arg(long)]
    index: Option<u16>,
}

impl GlvToken {
    pub(crate) fn address<C: Deref<Target = impl Signer> + Clone>(
        &self,
        client: &gmsol_sdk::Client<C>,
        store: &Pubkey,
    ) -> Pubkey {
        match (self.glv_token, self.index) {
            (Some(address), _) => address,
            (None, Some(index)) => client.find_glv_token_address(store, index),
            (None, None) => unreachable!(),
        }
    }
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
struct UpdateGlvArgs {
    /// Path to the update file (TOML).
    #[arg(long, short)]
    file: Option<PathBuf>,
    /// Minimum amount for the first GLV deposit.
    #[arg(long)]
    min_tokens_for_first_deposit: Option<GmAmount>,
    /// Minimum shift interval seconds.
    #[arg(long)]
    shift_min_interval_secs: Option<u32>,
    /// Maximum price impact factor after shift.
    #[arg(long)]
    shift_max_price_impact_factor: Option<Value>,
    /// Minimum shift value.
    #[arg(long)]
    shift_min_value: Option<Value>,
}

impl<'a> TryFrom<&'a UpdateGlvArgs> for UpdateGlvParams {
    type Error = eyre::Error;

    fn try_from(args: &'a UpdateGlvArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            min_tokens_for_first_deposit: args
                .min_tokens_for_first_deposit
                .map(|a| a.to_u64())
                .transpose()?,
            shift_min_interval_secs: args.shift_min_interval_secs,
            shift_max_price_impact_factor: args
                .shift_max_price_impact_factor
                .map(|v| v.to_u128())
                .transpose()?,
            shift_min_value: args.shift_min_value.map(|v| v.to_u128()).transpose()?,
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, clap::Args)]
#[group(required = true, multiple = true)]
struct MarketConfig {
    #[arg(long)]
    max_amount: Option<GmAmount>,
    #[arg(long)]
    max_value: Option<Value>,
}

impl MarketConfig {
    fn max_amount(&self) -> gmsol_sdk::Result<Option<u64>> {
        self.max_amount
            .as_ref()
            .map(|f| f.to_u64().map_err(gmsol_sdk::Error::custom))
            .transpose()
    }

    fn max_value(&self) -> gmsol_sdk::Result<Option<u128>> {
        self.max_value.as_ref().map(|f| f.to_u128()).transpose()
    }
}

#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MarketConfigWithFlag {
    #[serde(flatten)]
    config: MarketConfig,
    #[serde(flatten)]
    #[serde_as(as = "IndexMap<serde_with::DisplayFromStr, _>")]
    flag: IndexMap<GlvMarketFlag, bool>,
}

#[serde_with::serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GlvConfig {
    min_tokens_for_first_deposit: Option<GmAmount>,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    shift_min_interval: Option<humantime::Duration>,
    shift_max_price_impact_factor: Option<Value>,
    shift_min_value: Option<Value>,
}

impl TryFrom<GlvConfig> for UpdateGlvParams {
    type Error = gmsol_sdk::Error;

    fn try_from(config: GlvConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            min_tokens_for_first_deposit: config
                .min_tokens_for_first_deposit
                .map(|f| f.to_u64())
                .transpose()?,
            shift_min_interval_secs: config
                .shift_min_interval
                .map(|d| d.as_secs().try_into().map_err(gmsol_sdk::Error::custom))
                .transpose()?,
            shift_max_price_impact_factor: config
                .shift_max_price_impact_factor
                .map(|f| f.to_u128())
                .transpose()?,
            shift_min_value: config.shift_min_value.map(|f| f.to_u128()).transpose()?,
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UpdateGlv {
    #[serde(flatten)]
    glv: GlvConfig,
    #[serde(flatten)]
    market: IndexMap<StringPubkey, MarketConfigWithFlag>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glv_config_decimals_conversion() {
        let toml_content = r#"
            min_tokens_for_first_deposit = "1.2"
        "#;

        let config: GlvConfig = toml::from_str(toml_content).expect("Failed to parse TOML");
        let params: UpdateGlvParams = config.try_into().expect("Failed to convert");

        let expected = 1_200_000_000u64;
        assert_eq!(params.min_tokens_for_first_deposit, Some(expected));
    }
}
