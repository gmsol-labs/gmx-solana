use std::path::Path;

use tokio::{fs, io::AsyncWriteExt};

use crate::config::Config;

use super::Command;

/// Initialize config.
#[derive(Debug, clap::Args)]
pub struct InitConfig {
    /// Replace if the config file already exists.
    #[arg(long, short)]
    force: bool,
}

impl Command for InitConfig {
    async fn execute(&self, config_path: impl AsRef<Path>) -> eyre::Result<()> {
        if fs::try_exists(&config_path).await? && !self.force {
            eyre::bail!("Config file already exists. Use `--force` to overwrite it.");
        }

        if let Some(parent) = config_path.as_ref().parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(config_path)
            .await?;

        let default_config = Config::default();
        let content = toml::to_string_pretty(&default_config)?;
        file.write_all(content.as_bytes()).await?;

        Ok(())
    }
}
