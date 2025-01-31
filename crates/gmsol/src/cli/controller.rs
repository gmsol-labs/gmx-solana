use anchor_client::solana_sdk::pubkey::Pubkey;
use gmsol::{store::config::ConfigOps, utils::instruction::InstructionSerialization};
use gmsol_store::states::{AddressKey, Amount, AmountKey, Factor, FactorKey};

use crate::GMSOLClient;

#[derive(clap::Args)]
pub(super) struct ControllerArgs {
    #[command(subcommand)]
    command: Command,
}

#[allow(clippy::enum_variant_names)]
#[derive(clap::Subcommand)]
enum Command {
    /// Insert an amount to the config.
    InsertAmount {
        amount: Amount,
        #[arg(long, short)]
        key: AmountKey,
    },
    /// Insert a factor to the config.
    InsertFactor {
        factor: Factor,
        #[arg(long, short)]
        key: FactorKey,
    },
    /// Insert an address to the config.
    InsertAddress {
        address: Pubkey,
        #[arg(long, short)]
        key: AddressKey,
    },
}

impl ControllerArgs {
    pub(super) async fn run(
        &self,
        client: &GMSOLClient,
        store: &Pubkey,
        serialize_only: Option<InstructionSerialization>,
    ) -> gmsol::Result<()> {
        match &self.command {
            Command::InsertAmount { amount, key } => {
                crate::utils::send_or_serialize_rpc(
                    store,
                    client.insert_global_amount_by_key(store, *key, amount),
                    None,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::InsertFactor { factor, key } => {
                crate::utils::send_or_serialize_rpc(
                    store,
                    client.insert_global_factor_by_key(store, *key, factor),
                    None,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
            Command::InsertAddress { address, key } => {
                crate::utils::send_or_serialize_rpc(
                    store,
                    client.insert_global_address_by_key(store, *key, address),
                    None,
                    serialize_only,
                    true,
                    |signature| {
                        println!("{signature}");
                        Ok(())
                    },
                )
                .await?;
            }
        }
        Ok(())
    }
}
