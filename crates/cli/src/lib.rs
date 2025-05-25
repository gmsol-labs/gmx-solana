/// Configuration.
pub mod config;

/// Utils for wallet.
pub mod wallet;

/// Commands.
pub mod commands;

use std::{ops::Deref, path::PathBuf};

use clap::Parser;
use commands::{Command, CommandClient, Commands, Context};
use config::Config;
use eyre::OptionExt;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};

const ENV_PREFIX: &str = "GMSOL_";
const CONFIG_DIR: &str = "gmsol";

/// We use `__` in the name of environment variable as an alias of `.`.
///
/// See [`Env`] for more infomation.
const DOT_ALIAS: &str = "__";

/// Command-line interface for GMX-Solana.
#[derive(Debug)]
pub struct Cli(Inner);

impl Cli {
    /// Creates from the command line arguments.
    pub fn init() -> eyre::Result<Self> {
        let cli = Inner::parse();

        let config_path = cli.find_config()?;
        let Inner {
            config, command, ..
        } = cli;

        let config = Figment::new()
            .merge(Toml::file(config_path.clone()))
            .merge(Env::prefixed(ENV_PREFIX).split(DOT_ALIAS))
            .merge(Serialized::defaults(config))
            .extract()?;

        Ok(Self(Inner {
            config_path: Some(config_path),
            config,
            command,
        }))
    }
}

impl Deref for Cli {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Command-line interface for GMX-Solana.
#[derive(Debug, Parser)]
pub struct Inner {
    /// Path to the config file.
    #[clap(long = "config", short)]
    config_path: Option<PathBuf>,
    /// Config.
    #[command(flatten)]
    config: Config,
    /// Commands.
    #[command(subcommand)]
    command: Commands,
}

impl Inner {
    fn find_config(&self) -> eyre::Result<PathBuf> {
        use etcetera::{choose_base_strategy, BaseStrategy};

        match self.config_path.as_ref() {
            Some(path) => Ok(path.clone()),
            None => {
                let strategy = choose_base_strategy()?;
                Ok(strategy.config_dir().join(CONFIG_DIR).join("config.toml"))
            }
        }
    }

    /// Execute command.
    pub async fn execute(&self) -> eyre::Result<()> {
        let config_path = self
            .config_path
            .as_ref()
            .ok_or_eyre("config path is not set")?;
        #[cfg(feature = "remote-wallet")]
        let mut wallet_manager = None;
        let client = if self.command.is_client_required() {
            cfg_if::cfg_if! {
                if #[cfg(feature = "remote-wallet")] {
                    Some(CommandClient::new(&self.config, &mut wallet_manager)?)
                } else {
                    Some(CommandClient::new(&self.config)?)
                }
            }
        } else {
            None
        };
        let store = self.config.store_address();
        self.command
            .execute(Context::new(store, config_path, client.as_ref()))
            .await
    }
}
