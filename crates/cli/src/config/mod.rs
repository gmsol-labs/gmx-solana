mod output;
mod pyth;
mod store_address;

use std::num::NonZeroUsize;

use eyre::OptionExt;
use gmsol_sdk::{
    client::ClientOptions,
    pda,
    programs::anchor_lang::prelude::Pubkey,
    serde::StringPubkey,
    solana_utils::{
        bundle_builder::{BundleOptions, DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX},
        cluster::Cluster,
        compute_budget::ComputeBudget,
        signer::{local_signer, LocalSignerRef},
        solana_sdk::{
            commitment_config::{CommitmentConfig, CommitmentLevel},
            signature::NullSigner,
        },
    },
    utils::{instruction_serialization::InstructionSerialization, Lamport},
};
use store_address::StoreAddress;

use crate::wallet::signer_from_source;

pub use output::{DisplayOptions, OutputFormat};
pub use pyth::PythConfig;

cfg_if::cfg_if! {
    if #[cfg(feature = "devnet")] {
        const DEFAULT_CLUSTER: Cluster = Cluster::Devnet;
    } else {
        const DEFAULT_CLUSTER: Cluster = Cluster::Mainnet;
    }
}

const DEFAULT_WALLET: &str = "~/.config/solana/id.json";

const DEFAULT_COMMITMENT: CommitmentLevel = CommitmentLevel::Confirmed;

/// Configuration.
#[derive(Debug, clap::Args, serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct Config {
    /// Output format.
    #[arg(long, global = true)]
    output: Option<OutputFormat>,
    /// Path to the wallet.
    #[arg(long, short = 'k', global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    /// Cluster to connect to.
    #[arg(long = "url", short = 'u', global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cluster: Option<Cluster>,
    /// Commitment level.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commitment: Option<CommitmentLevel>,
    /// Store address.
    #[command(flatten)]
    #[serde(flatten)]
    store_address: StoreAddress,
    /// Store Program ID.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    store_program: Option<StringPubkey>,
    /// Treasury Program ID.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    treasury_program: Option<StringPubkey>,
    /// Timelock Program ID.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timelock_program: Option<StringPubkey>,
    /// Liquidity Provider Program ID.
    #[arg(long)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    liquidity_provider_program: Option<StringPubkey>,
    /// Print the serialized instructions,
    /// instead of sending the transaction.
    #[arg(long, global = true, default_missing_value = "base64", num_args=0..=1, group = "tx-opts")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    serialize_only: Option<InstructionSerialization>,
    /// Whether to skip preflight.
    #[arg(long, global = true, group = "tx-opts")]
    skip_preflight: bool,
    /// Use this address as payer.
    ///
    /// Only available in `serialize-only` mode.
    #[arg(long, requires = "serialize_only", global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payer: Option<StringPubkey>,
    /// Provides to create as timelocked instruction buffers.
    #[arg(long, group = "ix-buffer", global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timelock: Option<String>,
    /// Provides to create as a Squads vault transaction.
    #[cfg(feature = "squads")]
    #[arg(long, group = "ix-buffer", global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    squads: Option<String>,
    /// ALTs.
    #[arg(long, short = 't', global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alts: Option<Vec<StringPubkey>>,
    /// Oracle buffer to use.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oracle: Option<StringPubkey>,
    /// Max transaction size.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_transaction_size: Option<usize>,
    /// Force one transaction.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    force_one_tx: Option<bool>,
    /// Max instructions per transaction.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_transaction_instructions: Option<NonZeroUsize>,
    /// Priority fee lamports.
    #[arg(long, global = true)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    priority_lamports: Option<Lamport>,
    /// Chaos Labs Risk Oracle configuration.
    /// Not a CLI flag; only loaded from config file and env.
    #[arg(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chaos: Option<ChaosConfig>,
    /// Pyth Hermes configuration.
    /// Not a CLI flag; only loaded from config file and env.
    #[arg(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pyth: Option<PythConfig>,
}

impl Config {
    /// Creates a wallet based on the config.
    pub fn wallet(&self) -> eyre::Result<Payer> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "remote-wallet")] {
                self.create_wallet(None)
            } else {
                self.create_wallet()
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
    ) -> eyre::Result<Payer> {
        self.create_wallet(Some(wallet_manager))
    }

    pub(crate) fn create_wallet(
        &self,
        #[cfg(feature = "remote-wallet")] wallet_manager: Option<
            &mut Option<std::rc::Rc<solana_remote_wallet::remote_wallet::RemoteWalletManager>>,
        >,
    ) -> eyre::Result<Payer> {
        if let Some(payer) = self.payer {
            if self.serialize_only.is_some() {
                let payer = NullSigner::new(&payer);
                Ok(Payer::new(local_signer(payer)))
            } else {
                eyre::bail!("Setting payer is only allowed in `serialize-only` mode");
            }
        } else {
            let wallet = signer_from_source(
                self.wallet.as_deref().unwrap_or(DEFAULT_WALLET),
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

                return Ok(Payer::with_proposer(local_signer(payer), Some(wallet)));
            }

            #[cfg(feature = "squads")]
            if let Some(squads) = self.squads.as_ref() {
                let (multisig, vault_index) = parse_squads(squads)?;
                let vault_pda = gmsol_sdk::squads::get_vault_pda(&multisig, vault_index, None).0;

                let payer = NullSigner::new(&vault_pda);

                return Ok(Payer::with_proposer(local_signer(payer), Some(wallet)));
            }

            Ok(Payer::new(wallet))
        }
    }

    /// Returns the cluster.
    pub fn cluster(&self) -> &Cluster {
        self.cluster.as_ref().unwrap_or(&DEFAULT_CLUSTER)
    }

    /// Returns the client options.
    pub fn options(&self) -> ClientOptions {
        ClientOptions::builder()
            .commitment(CommitmentConfig {
                commitment: self.commitment.unwrap_or(DEFAULT_COMMITMENT),
            })
            .store_program_id(Some(*self.store_program_id()))
            .treasury_program_id(Some(*self.treasury_program_id()))
            .timelock_program_id(Some(*self.timelock_program_id()))
            .liquidity_provider_program_id(Some(*self.liquidity_provider_program_id()))
            .build()
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

    /// Returns the program ID of liquidity provider program.
    pub fn liquidity_provider_program_id(&self) -> &Pubkey {
        self.liquidity_provider_program
            .as_deref()
            .unwrap_or(&gmsol_sdk::programs::gmsol_liquidity_provider::ID)
    }

    /// Returns the address of the store account.
    pub fn store_address(&self) -> Pubkey {
        self.store_address.address(self.store_program_id())
    }

    /// Returns serialize-only option.
    pub fn serialize_only(&self) -> Option<InstructionSerialization> {
        self.serialize_only
    }

    /// Returns instruction buffer.
    pub fn ix_buffer(&self) -> eyre::Result<Option<InstructionBuffer>> {
        if let Some(role) = self.timelock.as_ref() {
            return Ok(Some(InstructionBuffer::Timelock { role: role.clone() }));
        }

        #[cfg(feature = "squads")]
        if let Some(squads) = self.squads.as_ref() {
            let (multisig, vault_index) = parse_squads(squads)?;
            return Ok(Some(InstructionBuffer::Squads {
                multisig,
                vault_index,
            }));
        }

        Ok(None)
    }

    /// Get oracle buffer address.
    pub fn oracle(&self) -> eyre::Result<&Pubkey> {
        self.oracle
            .as_deref()
            .ok_or_eyre("oracle buffer address is not provided")
    }

    /// Get address lookup tables.
    pub fn alts(&self) -> impl Iterator<Item = &Pubkey> {
        self.alts.iter().flat_map(|alts| alts.iter().map(|p| &p.0))
    }

    /// Get output format.
    pub fn output(&self) -> OutputFormat {
        self.output.unwrap_or_default()
    }

    /// Get bundle options.
    pub fn bundle_options(&self) -> BundleOptions {
        BundleOptions {
            force_one_transaction: self.force_one_tx.unwrap_or(false),
            max_packet_size: self.max_transaction_size,
            max_instructions_for_one_tx: self
                .max_transaction_instructions
                .map(|m| m.get())
                .unwrap_or(DEFAULT_MAX_INSTRUCTIONS_FOR_ONE_TX),
        }
    }

    /// Get priority lamports.
    pub fn priority_lamports(&self) -> eyre::Result<u64> {
        Ok(self
            .priority_lamports
            .map(|a| a.to_u64())
            .transpose()?
            .unwrap_or(ComputeBudget::DEFAULT_MIN_PRIORITY_LAMPORTS))
    }

    /// Returns whether the transaction preflight test should be skipped.
    pub fn skip_preflight(&self) -> bool {
        self.skip_preflight
    }

    pub fn chaos_base_url(&self) -> String {
        if let Ok(v) = std::env::var("CHAOS_BASE_URL") {
            return v;
        }
        self.chaos
            .as_ref()
            .and_then(|c| c.base_url.clone())
            .unwrap_or_else(|| "https://oracle.chaoslabs.co".to_string())
    }

    pub fn chaos_api_key(&self) -> Option<String> {
        if let Ok(v) = std::env::var("CHAOS_API_KEY") {
            return Some(v);
        }
        self.chaos.as_ref().and_then(|c| c.api_key.clone())
    }

    pub fn chaos_signer_strict(&self) -> eyre::Result<Option<Pubkey>> {
        if let Ok(v) = std::env::var("RISK_ORACLE_SIGNER") {
            let pk: Pubkey = v
                .parse()
                .map_err(|_| eyre::eyre!("invalid RISK_ORACLE_SIGNER: {v}"))?;
            return Ok(Some(pk));
        }
        Ok(self
            .chaos
            .as_ref()
            .and_then(|c| c.signer.as_ref().map(|s| s.0)))
    }

    /// Returns an authenticated Hermes client for commands that fetch Pyth prices.
    ///
    /// Errors with an actionable message when no API key is configured.
    #[cfg(feature = "pyth")]
    pub fn hermes(&self) -> eyre::Result<gmsol_sdk::client::pyth::Hermes> {
        self.optional_hermes()?
            .ok_or_eyre(pyth::MISSING_PYTH_API_KEY)
    }

    /// Returns an authenticated Hermes client, or `None` when no API key is configured.
    ///
    /// For callers that only sometimes need Pyth prices and check that themselves.
    #[cfg(feature = "pyth")]
    pub fn optional_hermes(&self) -> eyre::Result<Option<gmsol_sdk::client::pyth::Hermes>> {
        use gmsol_sdk::client::pyth::pull_oracle::hermes::ENV_API_KEY;

        let env_key = std::env::var(ENV_API_KEY).ok();
        self.hermes_with_env_key(env_key.as_deref())
    }

    /// Builds the Hermes client from the `[pyth]` section and the given `PYTH_API_KEY` value.
    #[cfg(feature = "pyth")]
    fn hermes_with_env_key(
        &self,
        env_key: Option<&str>,
    ) -> eyre::Result<Option<gmsol_sdk::client::pyth::Hermes>> {
        use gmsol_sdk::client::pyth::{pull_oracle::hermes::DEFAULT_HERMES_BASE, Hermes};

        let pyth = self.pyth.clone().unwrap_or_default();
        let Some(api_key) = pyth::resolve_api_key(pyth.api_key.as_deref(), env_key) else {
            return Ok(None);
        };
        let endpoint = pyth.endpoint.as_deref().unwrap_or(DEFAULT_HERMES_BASE);
        Ok(Some(Hermes::try_new_with_api_key(endpoint, api_key)?))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChaosConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<StringPubkey>,
}

#[cfg(feature = "squads")]
pub(crate) fn parse_squads(data: &str) -> eyre::Result<(Pubkey, u8)> {
    let (multisig, vault_index) = match data.split_once(':') {
        Some((multisig, vault_index)) => (multisig, vault_index.parse()?),
        None => (data, 0),
    };
    Ok((multisig.parse()?, vault_index))
}

/// Represents the entities involved in signing a transaction,
/// including the primary payer and an optional proposer.
#[derive(Debug, Clone)]
pub struct Payer {
    /// Payer.
    pub payer: LocalSignerRef,
    /// Proposer.
    pub proposer: Option<LocalSignerRef>,
}

impl Payer {
    fn with_proposer(payer: LocalSignerRef, proposer: Option<LocalSignerRef>) -> Self {
        Self { payer, proposer }
    }

    fn new(payer: LocalSignerRef) -> Self {
        Self::with_proposer(payer, None)
    }
}

/// Instruction Buffer.
pub enum InstructionBuffer {
    /// Timelock instruction buffer.
    Timelock { role: String },
    /// Squads instruction buffer.
    #[cfg(feature = "squads")]
    Squads { multisig: Pubkey, vault_index: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from_toml(toml: &str) -> Config {
        use figment::{
            providers::{Format, Serialized, Toml},
            Figment,
        };
        // Same layering as `Cli::init`, minus env vars.
        Figment::new()
            .merge(Serialized::defaults(Config::default()))
            .join(Toml::string(toml))
            .extract()
            .expect("valid config")
    }

    #[test]
    fn pyth_section_is_loaded_from_toml() {
        let config = config_from_toml(
            r#"
            [pyth]
            endpoint = "https://hermes.example/prefix"
            api_key = "cfg-key"
            "#,
        );
        let pyth = config.pyth.expect("pyth section");
        assert_eq!(
            pyth.endpoint.as_deref(),
            Some("https://hermes.example/prefix")
        );
        assert_eq!(pyth.api_key.as_deref(), Some("cfg-key"));
    }

    #[cfg(feature = "pyth")]
    #[test]
    fn hermes_uses_configured_endpoint() {
        let config = config_from_toml(
            r#"
            [pyth]
            endpoint = "https://hermes.example/prefix"
            api_key = "cfg-key"
            "#,
        );
        let hermes = config
            .hermes_with_env_key(None)
            .unwrap()
            .expect("key is configured");
        let printed = format!("{hermes:?}");
        // `Url`'s Debug prints host and path as separate fields.
        assert!(printed.contains("\"hermes.example\""));
        assert!(printed.contains("\"/prefix/\""));
        assert!(!printed.contains("cfg-key"));
    }

    #[cfg(feature = "pyth")]
    #[test]
    fn hermes_falls_back_to_env_key_and_default_endpoint() {
        let hermes = Config::default()
            .hermes_with_env_key(Some("env-key"))
            .unwrap()
            .expect("env key is set");
        assert!(format!("{hermes:?}").contains("hermes.pyth.network"));
    }

    #[cfg(feature = "pyth")]
    #[test]
    fn hermes_is_none_without_any_key() {
        assert!(Config::default()
            .hermes_with_env_key(None)
            .unwrap()
            .is_none());
        assert!(Config::default()
            .hermes_with_env_key(Some(""))
            .unwrap()
            .is_none());
    }

    #[cfg(feature = "pyth")]
    #[test]
    fn invalid_endpoint_is_an_error() {
        let config = config_from_toml(
            r#"
            [pyth]
            endpoint = "not a url"
            api_key = "cfg-key"
            "#,
        );
        assert!(config.hermes_with_env_key(None).is_err());
    }
}
