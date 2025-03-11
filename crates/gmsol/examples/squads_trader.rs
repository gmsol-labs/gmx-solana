use std::rc::Rc;

use clap::Parser;
use eyre::eyre;
use figment::{
    providers::{Env, Serialized},
    Figment,
};
use futures_util::TryFutureExt;
use gmsol::{
    exchange::ExchangeOps,
    solana_utils::cluster::Cluster,
    squads::{get_vault_pda, SquadsOps},
    utils::{builder::MakeBundleBuilder, local_signer, LocalSignerRef},
};
use serde_with::{serde_as, DisplayFromStr};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{pubkey::Pubkey, signature::NullSigner};
use tracing_subscriber::EnvFilter;

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize, Parser)]
struct Trader {
    #[clap(skip)]
    #[serde(skip)]
    wallet_manager: Option<Rc<RemoteWalletManager>>,
    #[arg(long = "url", short = 'u', default_value = "mainnet")]
    cluster: String,
    #[clap(long, default_value = "~/.config/solana/id.json")]
    proposer: String,
    #[clap(long)]
    #[serde_as(as = "DisplayFromStr")]
    multisig: Pubkey,
    #[clap(long, default_value_t = 0)]
    vault_index: u8,
    #[serde_as(as = "DisplayFromStr")]
    market_token: Pubkey,
    #[clap(long, short)]
    decrease: bool,
    #[clap(long, short)]
    amount: u64,
    #[clap(long, short)]
    size: String,
    #[clap(long)]
    #[serde(default)]
    approve: bool,
    #[clap(long)]
    #[serde(default)]
    execute: bool,
}

impl Trader {
    fn cluster(&self) -> eyre::Result<Cluster> {
        self.cluster
            .parse()
            .map_err(|err| eyre!("Invalid cluster: {err}"))
    }

    fn proposer_wallet(&mut self) -> eyre::Result<LocalSignerRef> {
        gmsol::cli::signer_from_source(&self.proposer, false, "keypair", &mut self.wallet_manager)
    }

    fn proposer(&mut self) -> eyre::Result<gmsol::Client<LocalSignerRef>> {
        let cluster = self.cluster()?;
        Ok(gmsol::Client::new(cluster, self.proposer_wallet()?)?)
    }

    fn multisig(&self) -> eyre::Result<gmsol::Client<LocalSignerRef>> {
        let cluster = self.cluster()?;
        let vault = get_vault_pda(&self.multisig, self.vault_index, None).0;
        let signer = NullSigner::new(&vault);
        Ok(gmsol::Client::new(cluster, local_signer(signer))?)
    }

    async fn run(&mut self) -> eyre::Result<()> {
        let proposer = self.proposer()?;
        let multisig = self.multisig()?;

        let store = multisig.find_store_address("");

        let (txn, order) = if self.decrease {
            multisig
                .market_decrease(
                    &store,
                    &self.market_token,
                    true,
                    self.amount,
                    true,
                    self.size.parse()?,
                )
                .build_with_address()
                .await?
        } else {
            multisig
                .market_increase(
                    &store,
                    &self.market_token,
                    true,
                    self.amount,
                    true,
                    self.size.parse()?,
                )
                .build_with_address()
                .await?
        };

        tracing::info!("creating order: {order}");

        let signatures = proposer
            .squads_from_bundle(&self.multisig, self.vault_index, txn)
            .approve(self.approve)
            .execute(self.execute)
            .build()
            .await?
            .send_all(true)
            .map_err(|(signatures, err)| {
                tracing::error!("partial success: {signatures:#?}");
                err
            })
            .await?;
        tracing::info!("success: {signatures:#?}");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let mut trader: Trader = Figment::new()
        .merge(Serialized::defaults(Trader::parse()))
        .merge(Env::prefixed("SQUADS_TRADER_"))
        .extract()?;
    trader.run().await?;
    Ok(())
}
