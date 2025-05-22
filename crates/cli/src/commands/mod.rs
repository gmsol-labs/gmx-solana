use std::path::Path;

use enum_dispatch::enum_dispatch;
use init_config::InitConfig;

mod init_config;

/// Commands.
#[enum_dispatch]
#[derive(Debug, clap::Subcommand)]
pub enum Commands {
    /// Initialize config file.
    InitConfig(InitConfig),
}

#[enum_dispatch(Commands)]
pub(crate) trait Command {
    async fn execute(&self, config_path: impl AsRef<Path>) -> eyre::Result<()>;
}
