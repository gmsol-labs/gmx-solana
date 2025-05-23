use std::{ops::Deref, path::Path};

use enum_dispatch::enum_dispatch;
use eyre::OptionExt;
use get_pubkey::GetPubkey;
use gmsol_sdk::{solana_utils::signer::LocalSignerRef, Client};
use init_config::InitConfig;

#[cfg(feature = "remote-wallet")]
use solana_remote_wallet::remote_wallet::RemoteWalletManager;

use crate::config::{Config, Payer};

mod get_pubkey;
mod init_config;

/// Commands.
#[enum_dispatch]
#[derive(Debug, clap::Subcommand)]
pub enum Commands {
    /// Initialize config file.
    InitConfig(InitConfig),
    /// Get pubkey of the payer.
    Pubkey(GetPubkey),
}

#[enum_dispatch(Commands)]
pub(crate) trait Command {
    fn is_client_required(&self) -> bool {
        false
    }

    async fn execute(&self, ctx: Context<'_>) -> eyre::Result<()>;
}

pub(crate) struct Context<'a> {
    config_path: &'a Path,
    client: Option<&'a CliClient>,
}

impl<'a> Context<'a> {
    pub(super) fn new(config_path: &'a Path, client: Option<&'a CliClient>) -> Self {
        Self {
            config_path,
            client,
        }
    }

    pub(crate) fn client(&self) -> eyre::Result<&CliClient> {
        self.client.ok_or_eyre("client is not provided")
    }
}

pub(crate) struct CliClient {
    client: Client<LocalSignerRef>,
    ix_buffer_client: Option<Client<LocalSignerRef>>,
}

impl CliClient {
    pub(crate) fn new(
        config: &Config,
        #[cfg(feature = "remote-wallet")] wallet_manager: &mut Option<
            std::rc::Rc<RemoteWalletManager>,
        >,
    ) -> eyre::Result<Self> {
        let Payer { payer, proposer } = config.create_wallet(
            #[cfg(feature = "remote-wallet")]
            Some(wallet_manager),
        )?;

        let cluster = config.cluster();
        let options = config.options();
        let client = Client::new_with_options(cluster.clone(), payer, options.clone())?;
        let ix_buffer_client = proposer
            .map(|payer| Client::new_with_options(cluster.clone(), payer, options))
            .transpose()?;

        Ok(Self {
            client,
            ix_buffer_client,
        })
    }
}

impl Deref for CliClient {
    type Target = Client<LocalSignerRef>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}
