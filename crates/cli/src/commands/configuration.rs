use gmsol_sdk::{
    core::config::{ActionDisabledFlag, AddressKey, AmountKey, DomainDisabledFlag, FactorKey},
    ops::config::ConfigOps,
    programs::anchor_lang::prelude::Pubkey,
    utils::Value,
};

/// On-chain configuration and features management.
#[derive(Debug, clap::Args)]
pub struct Configuration {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Insert an amount to the config.
    InsertAmount {
        amount: u64,
        #[arg(long)]
        key: AmountKey,
    },
    /// Insert a factor to the config.
    InsertFactor {
        factor: Value,
        #[arg(long)]
        key: FactorKey,
    },
    /// Insert an address to the config.
    InsertAddress {
        address: Pubkey,
        #[arg(long)]
        key: AddressKey,
    },
    /// Toggle feature.
    ToggleFeature {
        /// Feature domain.
        #[clap(requires = "toggle")]
        domain: DomainDisabledFlag,
        /// Feature action.
        action: ActionDisabledFlag,
        /// Enable the given feature.
        #[arg(long, group = "toggle")]
        enable: bool,
        /// Disable the given feature.
        #[arg(long, group = "toggle")]
        disable: bool,
    },
}

impl super::Command for Configuration {
    fn is_client_required(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: super::Context<'_>) -> eyre::Result<()> {
        let client = ctx.client()?;
        let store = ctx.store();
        let options = ctx.bundle_options();

        let bundle = match &self.command {
            Command::InsertAmount { amount, key } => {
                let builder = client.insert_global_amount_by_key(store, *key, amount);
                builder.into_bundle_with_options(options)?
            }
            Command::InsertFactor { factor, key } => {
                let builder = client.insert_global_factor_by_key(store, *key, &factor.to_u128()?);
                builder.into_bundle_with_options(options)?
            }
            Command::InsertAddress { address, key } => {
                let builder = client.insert_global_address_by_key(store, *key, address);
                builder.into_bundle_with_options(options)?
            }
            Command::ToggleFeature {
                domain,
                action,
                enable,
                disable,
            } => {
                if enable == disable {
                    return Err(eyre::eyre!("invalid toggle flags"));
                }
                let builder = client.toggle_feature(store, *domain, *action, *enable);
                builder.into_bundle_with_options(options)?
            }
        };

        client.send_or_serialize(bundle).await?;
        Ok(())
    }
}
