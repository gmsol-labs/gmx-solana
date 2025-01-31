use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    commitment_config::CommitmentConfig,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
    transaction::VersionedTransaction,
};

#[cfg(feature = "anchor")]
use anchor_lang::prelude::*;

use crate::{
    client::SendAndConfirm, cluster::Cluster, compute_budget::ComputeBudget, signer::BoxSigner,
};

/// Wallet Config.
#[derive(Clone)]
pub struct Config<C> {
    cluster: Cluster,
    payer: C,
    options: CommitmentConfig,
}

impl<C> Config<C> {
    /// Create a new wallet config.
    pub fn new(cluster: Cluster, payer: C, options: CommitmentConfig) -> Self {
        Self {
            cluster,
            payer,
            options,
        }
    }

    /// Get cluster.
    pub fn cluster(&self) -> &Cluster {
        &self.cluster
    }

    /// Get commitment config.
    pub fn commitment(&self) -> &CommitmentConfig {
        &self.options
    }

    /// Create a Solana RPC Client.
    pub fn rpc(&self) -> RpcClient {
        self.cluster.rpc(self.options)
    }

    /// Set payer.
    pub fn set_payer<C2>(self, payer: C2) -> Config<C2> {
        Config {
            cluster: self.cluster,
            payer,
            options: self.options,
        }
    }

    /// Set cluster.
    pub fn set_cluster(mut self, url: impl AsRef<str>) -> crate::Result<Self> {
        self.cluster = url.as_ref().parse()?;
        Ok(self)
    }

    /// Set options.
    pub fn set_options(mut self, options: CommitmentConfig) -> Self {
        self.options = options;
        self
    }
}

impl<C: Deref<Target = impl Signer>> Config<C> {
    /// Get payer pubkey.
    pub fn payer(&self) -> Pubkey {
        self.payer.pubkey()
    }
}

/// A builder for a transaction.
#[must_use = "transaction builder do nothing if not built"]
pub struct TransactionBuilder<'a, C, T = ()> {
    output: T,
    program_id: Pubkey,
    cfg: Config<C>,
    signers: Vec<&'a dyn Signer>,
    owned_signers: Vec<BoxSigner>,
    pre_instructions: Vec<Instruction>,
    accounts: Vec<AccountMeta>,
    instruction_data: Option<Vec<u8>>,
    compute_budget: ComputeBudget,
    luts: HashMap<Pubkey, Vec<Pubkey>>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> TransactionBuilder<'a, C> {
    /// Create a new transaction builder.
    pub fn new(program_id: Pubkey, cfg: &'a Config<C>) -> Self {
        Self {
            output: (),
            program_id,
            cfg: cfg.clone(),
            signers: Default::default(),
            owned_signers: Default::default(),
            pre_instructions: Default::default(),
            accounts: Default::default(),
            instruction_data: None,
            compute_budget: ComputeBudget::default(),
            luts: Default::default(),
        }
    }

    /// Merge other [`TransactionBuilder`]. The rpc fields will be empty after merging,
    /// i.e., `take_rpc` will return `None`.
    /// ## Panics
    /// Return if there are any errors.
    /// ## Notes
    /// - All options including `cluster`, `commiment` and `program_id` will still be
    ///   the same of `self` after merging.
    #[inline]
    pub fn merge(mut self, mut other: Self) -> Self {
        self.try_merge(&mut other)
            .unwrap_or_else(|err| panic!("failed to merge: {err}"));
        self
    }

    /// Merge other [`TransactionBuilder`]. The rpc fields will be empty after merging,
    /// i.e., `take_rpc` will return `None`.
    /// ## Errors
    /// Return error if the `payer`s are not the same.
    /// ## Notes
    /// - When success, the `other` will become a empty [`TransactionBuilder`].
    /// - All options including `cluster`, `commiment`, and `program_id` will still be
    ///   the same of `self` after merging.
    pub fn try_merge(&mut self, other: &mut Self) -> crate::Result<()> {
        if self.cfg.payer() != other.cfg.payer() {
            return Err(crate::Error::MergeTransaction("payer mismatched"));
        }

        // Push the rpc ix before merging.
        if let Some(ix) = self.take_instruction() {
            self.pre_instructions.push(ix);
        }

        // Merge ixs.
        self.pre_instructions.append(&mut other.pre_instructions);
        if let Some(ix) = other.take_instruction() {
            self.pre_instructions.push(ix);
        }

        // Merge signers.
        self.signers.append(&mut other.signers);

        // Merge owned signers.
        self.owned_signers.append(&mut other.owned_signers);

        // Merge compute budget.
        self.compute_budget += std::mem::take(&mut other.compute_budget);

        // Merge LUTs.
        self.luts.extend(other.luts.drain());
        Ok(())
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone, T> TransactionBuilder<'a, C, T> {
    /// Set payer.
    pub fn payer<C2>(self, payer: C2) -> TransactionBuilder<'a, C2, T> {
        TransactionBuilder {
            output: self.output,
            program_id: self.program_id,
            cfg: self.cfg.set_payer(payer),
            signers: self.signers,
            owned_signers: self.owned_signers,
            pre_instructions: self.pre_instructions,
            accounts: self.accounts,
            instruction_data: self.instruction_data,
            compute_budget: self.compute_budget,
            luts: self.luts,
        }
    }

    /// Get the pubkey of the payer.
    pub fn get_payer(&self) -> Pubkey {
        self.cfg.payer()
    }

    /// Set cluster.
    pub fn cluster(mut self, url: impl AsRef<str>) -> crate::Result<Self> {
        self.cfg = self.cfg.set_cluster(url)?;
        Ok(self)
    }

    /// Set commiment options.
    pub fn options(mut self, options: CommitmentConfig) -> Self {
        self.cfg = self.cfg.set_options(options);
        self
    }

    /// Add a signer to the signer list.
    pub fn signer(mut self, signer: &'a dyn Signer) -> Self {
        self.signers.push(signer);
        self
    }

    /// Add a owned sigenr to the signer list.
    pub fn owned_signer(mut self, signer: impl Signer + 'static) -> Self {
        self.owned_signers.push(Box::new(signer));
        self
    }

    /// Set program id.
    pub fn program(mut self, program_id: Pubkey) -> Self {
        self.program_id = program_id;
        self
    }

    /// Append accounts for the main instruction.
    pub fn accounts(mut self, mut accounts: Vec<AccountMeta>) -> Self {
        self.accounts.append(&mut accounts);
        self
    }

    /// Append accounts for the main instruction.
    #[cfg(feature = "anchor")]
    pub fn anchor_accounts(self, accounts: impl ToAccountMetas) -> Self {
        self.accounts(accounts.to_account_metas(None))
    }

    /// Set arguments for the main instruction.
    pub fn args(mut self, args: Vec<u8>) -> Self {
        self.instruction_data = Some(args);
        self
    }

    /// Set arguments for the main instruction.
    #[cfg(feature = "anchor")]
    pub fn anchor_args(self, args: impl anchor_lang::InstructionData) -> Self {
        self.args(args.data())
    }

    /// Set compute budget.
    pub fn compute_budget(mut self, budget: ComputeBudget) -> Self {
        self.compute_budget = budget;
        self
    }

    /// Get mutable reference to the compute budget.
    pub fn compute_budget_mut(&mut self) -> &mut ComputeBudget {
        &mut self.compute_budget
    }

    fn get_compute_budget_instructions(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> Vec<Instruction> {
        self.compute_budget
            .compute_budget_instructions(compute_unit_price_micro_lamports)
    }

    /// Take and construct the "main" instruction if present.
    pub fn take_instruction(&mut self) -> Option<Instruction> {
        let ix_data = self.instruction_data.take()?;
        Some(Instruction {
            program_id: self.program_id,
            data: ix_data,
            accounts: std::mem::take(&mut self.accounts),
        })
    }

    /// Construct the "main" instruction if present.
    fn get_instruction(&self) -> Option<Instruction> {
        let ix_data = self.instruction_data.as_ref()?;
        Some(Instruction {
            program_id: self.program_id,
            data: ix_data.clone(),
            accounts: self.accounts.clone(),
        })
    }

    /// Construct all instructions.
    pub fn instructions(&self) -> Vec<Instruction> {
        self.instructions_with_options(false, None)
    }

    /// Construct all instructions with options.
    pub fn instructions_with_options(
        &self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> Vec<Instruction> {
        let mut instructions = if without_compute_budget {
            Vec::default()
        } else {
            self.get_compute_budget_instructions(compute_unit_price_micro_lamports)
        };
        instructions.append(&mut self.pre_instructions.clone());
        if let Some(ix) = self.get_instruction() {
            instructions.push(ix);
        }
        instructions
    }

    /// Get the output.
    pub fn get_output(&self) -> &T {
        &self.output
    }

    /// Set the output and return the previous.
    pub fn swap_output<U>(self, output: U) -> (TransactionBuilder<'a, C, U>, T) {
        let Self {
            cfg,
            signers,
            owned_signers,
            output: previous,
            program_id,
            pre_instructions,
            accounts,
            instruction_data,
            compute_budget,
            luts,
        } = self;

        (
            TransactionBuilder {
                cfg,
                signers,
                owned_signers,
                output,
                program_id,
                pre_instructions,
                accounts,
                instruction_data,
                compute_budget,
                luts,
            },
            previous,
        )
    }

    /// Set the output.
    pub fn output<U>(self, output: U) -> TransactionBuilder<'a, C, U> {
        self.swap_output(output).0
    }

    /// Clear the output.
    pub fn clear_output(self) -> TransactionBuilder<'a, C, ()> {
        self.swap_output(()).0
    }

    /// Insert an instruction before the "main" instruction.
    pub fn pre_instruction(mut self, ix: Instruction) -> Self {
        self.pre_instructions.push(ix);
        self
    }

    /// Insert instructions before the "main" instruction.
    pub fn pre_instructions(mut self, mut ixs: Vec<Instruction>) -> Self {
        self.pre_instructions.append(&mut ixs);
        self
    }

    /// Insert an address lookup table account.
    pub fn lookup_table(mut self, account: AddressLookupTableAccount) -> Self {
        self.luts.insert(account.key, account.addresses);
        self
    }

    /// Insert many address lookup tables.
    pub fn lookup_tables(
        mut self,
        tables: impl IntoIterator<Item = (Pubkey, Vec<Pubkey>)>,
    ) -> Self {
        self.luts.extend(tables);
        self
    }

    fn v0_message_with_blockhash_and_options(
        &self,
        latest_hash: Hash,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
        with_luts: bool,
    ) -> crate::Result<v0::Message> {
        let instructions = self
            .instructions_with_options(without_compute_budget, compute_unit_price_micro_lamports);
        let luts = if with_luts {
            self.luts
                .iter()
                .map(|(key, addresses)| AddressLookupTableAccount {
                    key: *key,
                    addresses: addresses.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let message =
            v0::Message::try_compile(&self.cfg.payer(), &instructions, &luts, latest_hash)?;

        Ok(message)
    }

    /// Get versioned message with the given hash and options.
    pub fn message_with_blockhash_and_options(
        &self,
        latest_hash: Hash,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<VersionedMessage> {
        Ok(VersionedMessage::V0(
            self.v0_message_with_blockhash_and_options(
                latest_hash,
                without_compute_budget,
                compute_unit_price_micro_lamports,
                true,
            )?,
        ))
    }

    /// Get versioned message with options.
    pub async fn message_with_options(
        &self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<VersionedMessage> {
        let client = self.cfg.rpc();
        let latest_hash = client.get_latest_blockhash().await.map_err(Box::new)?;

        self.message_with_blockhash_and_options(
            latest_hash,
            without_compute_budget,
            compute_unit_price_micro_lamports,
        )
    }

    /// Get signed transaction with blockhash and options.
    pub fn signed_transaction_with_blockhash_and_options(
        &self,
        latest_hash: Hash,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<VersionedTransaction> {
        let message = self.message_with_blockhash_and_options(
            latest_hash,
            without_compute_budget,
            compute_unit_price_micro_lamports,
        )?;

        let mut signers = self.signers.clone();
        signers.push(&*self.cfg.payer);
        for signer in self.owned_signers.iter() {
            signers.push(&**signer);
        }

        let tx = VersionedTransaction::try_new(message, &signers)?;

        Ok(tx)
    }

    /// Get signed transactoin with options.
    pub async fn signed_transaction_with_options(
        &self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<VersionedTransaction> {
        let client = self.cfg.rpc();
        let latest_hash = client.get_latest_blockhash().await.map_err(Box::new)?;

        self.signed_transaction_with_blockhash_and_options(
            latest_hash,
            without_compute_budget,
            compute_unit_price_micro_lamports,
        )
    }

    /// Sign and send the transaction with options.
    pub async fn send_with_options(
        &self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
        mut config: RpcSendTransactionConfig,
    ) -> crate::Result<Signature> {
        let client = self.cfg.rpc();
        let latest_hash = client.get_latest_blockhash().await.map_err(Box::new)?;

        let tx = self.signed_transaction_with_blockhash_and_options(
            latest_hash,
            without_compute_budget,
            compute_unit_price_micro_lamports,
        )?;

        config.preflight_commitment = config
            .preflight_commitment
            .or(Some(client.commitment().commitment));

        let signature = client
            .send_and_confirm_transaction_with_config(&tx, config)
            .await
            .map_err(Box::new)?;

        Ok(signature)
    }

    /// Build and send the transaction without preflight.
    pub async fn send_without_preflight(self) -> crate::Result<Signature> {
        self.send_with_options(
            false,
            None,
            RpcSendTransactionConfig {
                skip_preflight: true,
                ..Default::default()
            },
        )
        .await
    }

    /// Build and send the transaction with default options.
    pub async fn send(self) -> crate::Result<Signature> {
        self.send_with_options(false, None, Default::default())
            .await
    }

    /// Get complete lookup table.
    pub fn get_complete_lookup_table(&self) -> HashSet<Pubkey> {
        self.luts
            .values()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
    }

    /// Get luts.
    pub fn get_luts(&self) -> &HashMap<Pubkey, Vec<Pubkey>> {
        &self.luts
    }

    /// Estimated the size of the result transaction.
    ///
    /// See [`transaction_size()`](crate::utils::transaction_size()) for more information.
    pub fn transaction_size(&self, is_versioned_transaction: bool) -> usize {
        let lookup_table = self.get_complete_lookup_table();
        crate::utils::transaction_size(
            &self.instructions(),
            is_versioned_transaction,
            Some(&lookup_table),
            self.get_luts().len(),
        )
    }

    /// Estimated the execution fee of the result transaction.
    pub async fn estimate_execution_fee(
        &self,
        _client: &RpcClient,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<u64> {
        let ixs = self.instructions_with_options(true, None);
        let mut compute_budget = self.compute_budget;
        if let Some(price) = compute_unit_price_micro_lamports {
            compute_budget = compute_budget.with_price(price);
        }
        let num_signers = ixs
            .iter()
            .flat_map(|ix| ix.accounts.iter())
            .filter(|meta| meta.is_signer)
            .map(|meta| &meta.pubkey)
            .collect::<HashSet<_>>()
            .len() as u64;
        let fee = num_signers * 5_000 + compute_budget.fee();
        Ok(fee)
    }
}
