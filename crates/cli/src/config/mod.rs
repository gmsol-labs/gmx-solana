mod store_address;

use gmsol_sdk::{
    pda,
    programs::anchor_lang::prelude::Pubkey,
    solana_utils::{
        cluster::Cluster,
        signer::{local_signer, LocalSignerRef},
        solana_sdk::{commitment_config::CommitmentLevel, signature::NullSigner},
    },
    utils::{instruction_serialization::InstructionSerialization, serde::StringPubkey},
};
use store_address::StoreAddress;

use crate::wallet::signer_from_source;

cfg_if::cfg_if! {
    if #[cfg(feature = "devnet")] {
        const DEFAULT_CLUSTER: &str = "devnet";
    } else {
        const DEFAULT_CLUSTER: &str = "mainnet";
    }
}

const DEFAULT_WALLET: &str = "~/.config/solana/id.json";

/// Configuration.
#[derive(Debug, clap::Args, serde::Serialize, serde::Deserialize, Clone)]
pub struct Config {
    /// Path to the wallet.
    #[arg(long, short, env, default_value = DEFAULT_WALLET)]
    wallet: String,
    /// Cluster to connect to.
    #[arg(long = "url", short = 'u', env, default_value = DEFAULT_CLUSTER)]
    cluster: Cluster,
    /// Commitment level.
    #[arg(long, env, default_value_t = CommitmentLevel::Confirmed)]
    commitment: CommitmentLevel,
    /// Store address.
    #[command(flatten)]
    #[serde(flatten)]
    store_address: StoreAddress,
    /// Store Program ID.
    #[arg(long, env)]
    store_program: Option<StringPubkey>,
    /// Treasury Program ID.
    #[arg(long, env)]
    treasury_program: Option<StringPubkey>,
    /// Timelock Program ID.
    #[arg(long, env)]
    timelock_program: Option<StringPubkey>,
    /// Print the serialized instructions,
    /// instead of sending the transaction.
    #[arg(long)]
    serialize_only: Option<InstructionSerialization>,
    /// Use this address as payer.
    ///
    /// Only available in `serialize-only` mode.
    #[arg(long, requires = "serialize_only")]
    payer: Option<StringPubkey>,
    /// Whether to create a timelocked buffer for this instruction.
    #[arg(long, group = "ix-buffer")]
    timelock: Option<String>,
    #[cfg(feature = "squads")]
    #[cfg_attr(feature = "squads", arg(long, group = "ix-buffer"))]
    squads: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wallet: DEFAULT_WALLET.to_string(),
            cluster: Cluster::Mainnet,
            commitment: CommitmentLevel::Confirmed,
            store_address: Default::default(),
            store_program: None,
            treasury_program: None,
            timelock_program: None,
            serialize_only: None,
            payer: None,
            timelock: None,
            #[cfg(feature = "squads")]
            squads: None,
        }
    }
}

impl Config {
    /// Creates a wallet based on the config.
    pub fn wallet(&self) -> eyre::Result<LocalSignerRef> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "remote-wallet")] {
                Ok(self.create_wallet(None)?.0)
            } else {
                Ok(self.create_wallet()?.0)
            }
        }
    }

    /// Creates a wallet based on the config.
    /// Supports remote wallets.
    #[cfg(feature = "remote-wallet")]
    pub fn wallet_with_remote_support(
        &self,
        wallet_manager: &mut Option<
            std::rc::Rc<solana_remote_wallet::remote_wallet::RemoteWalletManager>,
        >,
    ) -> eyre::Result<(LocalSignerRef, Option<LocalSignerRef>)> {
        self.create_wallet(Some(wallet_manager))
    }

    fn create_wallet(
        &self,
        #[cfg(feature = "remote-wallet")] wallet_manager: Option<
            &mut Option<std::rc::Rc<solana_remote_wallet::remote_wallet::RemoteWalletManager>>,
        >,
    ) -> eyre::Result<(LocalSignerRef, Option<LocalSignerRef>)> {
        if let Some(payer) = self.payer {
            if self.serialize_only.is_some() {
                let payer = NullSigner::new(&payer);
                Ok((local_signer(payer), None))
            } else {
                eyre::bail!("Setting payer is only allowed in `serialize-only` mode");
            }
        } else {
            let wallet = signer_from_source(
                &self.wallet,
                #[cfg(feature = "remote-wallet")]
                false,
                #[cfg(feature = "remote-wallet")]
                "keypair",
                #[cfg(feature = "remote-wallet")]
                wallet_manager,
            )?;

            if let Some(role) = self.timelock.as_ref() {
                let store = self.store_address();
                let timelock_program_id = self.timelock_program_id();
                let executor = pda::find_executor_address(
                    &store,
                    role,
                    self.timelock_program
                        .as_deref()
                        .unwrap_or(timelock_program_id),
                )?
                .0;
                let executor_wallet =
                    pda::find_executor_wallet_address(&executor, timelock_program_id).0;

                let payer = NullSigner::new(&executor_wallet);

                return Ok((local_signer(payer), Some(wallet)));
            }

            #[cfg(feature = "squads")]
            if let Some(squads) = self.squads.as_ref() {
                let (multisig, vault_index) = parse_squads(squads)?;
                let vault_pda = gmsol_sdk::squads::get_vault_pda(&multisig, vault_index, None).0;

                let payer = NullSigner::new(&vault_pda);

                return Ok((local_signer(payer), Some(wallet)));
            }

            Ok((wallet, None))
        }
    }

    /// Returns the program ID of store program.
    pub fn store_program_id(&self) -> &Pubkey {
        self.store_program
            .as_deref()
            .unwrap_or(&gmsol_sdk::programs::gmsol_store::ID)
    }

    /// Returns the program ID of treasury program.
    pub fn treasury_program_id(&self) -> &Pubkey {
        self.treasury_program
            .as_deref()
            .unwrap_or(&gmsol_sdk::programs::gmsol_treasury::ID)
    }

    /// Returns the program ID of timelock program.
    pub fn timelock_program_id(&self) -> &Pubkey {
        self.timelock_program
            .as_deref()
            .unwrap_or(&gmsol_sdk::programs::gmsol_timelock::ID)
    }

    /// Returns the address of the store account.
    pub fn store_address(&self) -> Pubkey {
        self.store_address.address(self.store_program_id())
    }
}

#[cfg(feature = "squads")]
fn parse_squads(data: &str) -> eyre::Result<(Pubkey, u8)> {
    let (multisig, vault_index) = match data.split_once(':') {
        Some((multisig, vault_index)) => (multisig, vault_index.parse()?),
        None => (data, 0),
    };
    Ok((multisig.parse()?, vault_index))
}
