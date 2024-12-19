use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use anchor_client::{
    anchor_lang::{InstructionData, ToAccountMetas},
    solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig},
    solana_sdk::{
        address_lookup_table::AddressLookupTableAccount,
        commitment_config::CommitmentConfig,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::{v0, VersionedMessage},
        pubkey::Pubkey,
        signature::Signature,
        signer::Signer,
        transaction::VersionedTransaction,
    },
    Cluster, RequestBuilder,
};

use super::{compute_budget::ComputeBudget, transaction_size::transaction_size, SendAndConfirm};

/// A builder of RPC request to Solana.
#[must_use]
pub struct RpcBuilder<'a, C, T = ()> {
    output: T,
    program_id: Pubkey,
    cfg: Config<C>,
    signers: Vec<&'a dyn Signer>,
    owned_signers: Vec<Box<dyn Signer>>,
    pre_instructions: Vec<Instruction>,
    accounts: Vec<AccountMeta>,
    instruction_data: Option<Vec<u8>>,
    compute_budget: ComputeBudget,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Wallet Config.
#[derive(Clone)]
pub struct Config<C> {
    cluster: Cluster,
    payer: C,
    options: CommitmentConfig,
}

impl<C> Config<C> {
    pub(crate) fn new(cluster: Cluster, payer: C, options: CommitmentConfig) -> Self {
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
        RpcClient::new_with_commitment(self.cluster.url().to_string(), self.options)
    }
}

impl<C: Deref<Target = impl Signer>> Config<C> {
    /// Get payer pubkey.
    pub fn payer(&self) -> Pubkey {
        self.payer.pubkey()
    }
}

/// Program.
pub struct Program<C> {
    program_id: Pubkey,
    cfg: Config<C>,
}

impl<C> Program<C> {
    pub fn new(program_id: Pubkey, cfg: Config<C>) -> Self {
        Self { program_id, cfg }
    }

    /// Create a Solana RPC Client.
    pub fn solana_rpc(&self) -> RpcClient {
        self.cfg.rpc()
    }

    /// Get the program id.
    pub fn id(&self) -> &Pubkey {
        &self.program_id
    }
}

impl<C: Deref<Target = impl Signer> + Clone> Program<C> {
    /// Create a [`RpcBuilder`].
    pub fn rpc(&self) -> RpcBuilder<C> {
        RpcBuilder::new(self.program_id, &self.cfg)
    }
}

impl<C: Deref<Target = impl Signer>> Program<C> {
    /// Get the pubkey of the payer.
    pub fn payer(&self) -> Pubkey {
        self.cfg.payer()
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> RpcBuilder<'a, C> {
    /// Create a new [`RpcBuilder`] from [`Program`].
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
            alts: Default::default(),
        }
    }

    /// Take and construct the RPC instruction if present.
    pub fn take_rpc(&mut self) -> Option<Instruction> {
        let ix_data = self.instruction_data.take()?;
        Some(Instruction {
            program_id: self.program_id,
            data: ix_data,
            accounts: std::mem::take(&mut self.accounts),
        })
    }

    /// Merge other [`RpcBuilder`]. The rpc fields will be empty after merging,
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

    /// Merge other [`RpcBuilder`]. The rpc fields will be empty after merging,
    /// i.e., `take_rpc` will return `None`.
    /// ## Errors
    /// Return error if the `payer`s are not the same.
    /// ## Notes
    /// - When success, the `other` will become a empty [`RpcBuilder`].
    /// - All options including `cluster`, `commiment`, and `program_id` will still be
    ///   the same of `self` after merging.
    pub fn try_merge(&mut self, other: &mut Self) -> crate::Result<()> {
        if self.cfg.payer.pubkey() != other.cfg.payer.pubkey() {
            return Err(crate::Error::invalid_argument("payer mismatched"));
        }

        // Push the rpc ix before merging.
        if let Some(ix) = self.take_rpc() {
            self.pre_instructions.push(ix);
        }

        // Merge ixs.
        self.pre_instructions.append(&mut other.pre_instructions);
        if let Some(ix) = other.take_rpc() {
            self.pre_instructions.push(ix);
        }

        // Merge signers.
        self.signers.append(&mut other.signers);

        // Merge owned signers.
        self.owned_signers.append(&mut other.owned_signers);

        // Merge compute budget.
        self.compute_budget += std::mem::take(&mut other.compute_budget);

        // Merge ALTs.
        self.alts.extend(other.alts.drain());
        Ok(())
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone, T> RpcBuilder<'a, C, T> {
    /// Set payer.
    pub fn payer<C2>(self, payer: C2) -> RpcBuilder<'a, C2, T> {
        RpcBuilder {
            output: self.output,
            program_id: self.program_id,
            cfg: Config {
                cluster: self.cfg.cluster,
                payer,
                options: self.cfg.options,
            },
            signers: self.signers,
            owned_signers: self.owned_signers,
            pre_instructions: self.pre_instructions,
            accounts: self.accounts,
            instruction_data: self.instruction_data,
            compute_budget: self.compute_budget,
            alts: self.alts,
        }
    }

    /// Set cluster.
    pub fn cluster(mut self, url: &str) -> crate::Result<Self> {
        self.cfg.cluster = url.parse().map_err(crate::Error::invalid_argument)?;
        Ok(self)
    }

    /// Set commiment options.
    pub fn options(mut self, options: CommitmentConfig) -> Self {
        self.cfg.options = options;
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

    /// Set accounts for the rpc method.
    pub fn accounts(mut self, accounts: impl ToAccountMetas) -> Self {
        let mut metas = accounts.to_account_metas(None);
        self.accounts.append(&mut metas);
        self
    }

    /// Set arguments for the rpc method.
    pub fn args(mut self, args: impl InstructionData) -> Self {
        let data = args.data();
        self.instruction_data = Some(data);
        self
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

    fn get_rpc_instruction(&self) -> Option<Instruction> {
        let ix_data = self.instruction_data.as_ref()?;
        Some(Instruction {
            program_id: self.program_id,
            data: ix_data.clone(),
            accounts: self.accounts.clone(),
        })
    }

    /// Get all instructions.
    pub fn instructions(&self) -> Vec<Instruction> {
        self.instructions_with_options(false, None)
    }

    /// Get all instructions with options.
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
        if let Some(ix) = self.get_rpc_instruction() {
            instructions.push(ix);
        }
        instructions
    }

    /// Get the output.
    pub fn output(&self) -> &T {
        &self.output
    }

    /// Set the output and return the previous.
    pub fn swap_output<U>(self, output: U) -> (RpcBuilder<'a, C, U>, T) {
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
            alts,
        } = self;

        (
            RpcBuilder {
                cfg,
                signers,
                owned_signers,
                output,
                program_id,
                pre_instructions,
                accounts,
                instruction_data,
                compute_budget,
                alts,
            },
            previous,
        )
    }

    /// Set the output.
    pub fn with_output<U>(self, output: U) -> RpcBuilder<'a, C, U> {
        self.swap_output(output).0
    }

    /// Clear the output.
    pub fn clear_output(self) -> RpcBuilder<'a, C, ()> {
        self.swap_output(()).0
    }

    /// Insert an instruction before the rpc method.
    pub fn pre_instruction(mut self, ix: Instruction) -> Self {
        self.pre_instructions.push(ix);
        self
    }

    /// Insert instructions before the rpc method.
    pub fn pre_instructions(mut self, mut ixs: Vec<Instruction>) -> Self {
        self.pre_instructions.append(&mut ixs);
        self
    }

    /// Insert an address lookup table account.
    pub fn lookup_table(mut self, account: AddressLookupTableAccount) -> Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    /// Insert many address lookup tables.
    pub fn lookup_tables(
        mut self,
        tables: impl IntoIterator<Item = (Pubkey, Vec<Pubkey>)>,
    ) -> Self {
        self.alts.extend(tables);
        self
    }

    fn v0_message_with_blockhash_and_options(
        &self,
        latest_hash: Hash,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
        with_alts: bool,
    ) -> crate::Result<v0::Message> {
        let instructions = self
            .instructions_with_options(without_compute_budget, compute_unit_price_micro_lamports);
        let alts = if with_alts {
            self.alts
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
            v0::Message::try_compile(&self.cfg.payer(), &instructions, &alts, latest_hash)?;

        Ok(message)
    }

    fn message_with_blockhash_and_options(
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

    /// Get compiled message with options.
    pub async fn message_with_options(
        &self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<VersionedMessage> {
        let client = self.cfg.rpc();
        let latest_hash = client
            .get_latest_blockhash()
            .await
            .map_err(anchor_client::ClientError::from)?;

        self.message_with_blockhash_and_options(
            latest_hash,
            without_compute_budget,
            compute_unit_price_micro_lamports,
        )
    }

    fn signed_transaction_with_blockhash_and_options(
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
        let latest_hash = client
            .get_latest_blockhash()
            .await
            .map_err(anchor_client::ClientError::from)?;

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
        let latest_hash = client
            .get_latest_blockhash()
            .await
            .map_err(anchor_client::ClientError::from)?;

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
            .map_err(anchor_client::ClientError::from)?;

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

    /// Build [`RequestBuilder`].
    pub fn into_anchor_request(self) -> RequestBuilder<'a, C> {
        self.into_anchor_request_with_options(false, None).0
    }

    /// Build [`RequestBuilder`] without compute budget.
    pub fn into_anchor_request_without_compute_budget(self) -> RequestBuilder<'a, C> {
        self.into_anchor_request_with_options(true, None).0
    }

    /// Build [`RequestBuilder`] and output.
    ///
    /// # Warning
    /// Owned signers will be ignored currently.
    pub fn into_anchor_request_with_options(
        self,
        without_compute_budget: bool,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> (RequestBuilder<'a, C>, T) {
        let request = anchor_client::RequestBuilder::from(
            self.program_id,
            self.cfg.cluster.url(),
            self.cfg.payer.clone(),
            Some(self.cfg.options),
        );
        let request = self
            .instructions_with_options(without_compute_budget, compute_unit_price_micro_lamports)
            .into_iter()
            .fold(request, |acc, ix| acc.instruction(ix));
        let request = self
            .signers
            .into_iter()
            .fold(request, |acc, signer| acc.signer(signer));
        (request, self.output)
    }

    /// Get complete lookup table.
    pub fn get_complete_lookup_table(&self) -> HashSet<Pubkey> {
        self.alts
            .values()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
    }

    /// Get alts.
    pub fn get_alts(&self) -> &HashMap<Pubkey, Vec<Pubkey>> {
        &self.alts
    }

    /// Estimated the size of the result transaction.
    ///
    /// See [`transaction_size()`] for more information.
    pub fn transaction_size(&self, is_versioned_transaction: bool) -> usize {
        let lookup_table = self.get_complete_lookup_table();
        transaction_size(
            &self.instructions(),
            is_versioned_transaction,
            Some(&lookup_table),
            self.get_alts().len(),
        )
    }

    /// Estimated the execution fee of the result transaction.
    pub async fn estimate_execution_fee(
        &self,
        _client: &RpcClient,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> crate::Result<u64> {
        // let blockhash = client
        //     .get_latest_blockhash()
        //     .await
        //     .map_err(anchor_client::ClientError::from)?;
        // // FIXME: we currently ignore the ALTs when estimating execution fee to avoid the
        // // "index out of bound" error returned by the RPC with ALTs provided.
        // let message = self.v0_message_with_blockhash_and_options(
        //     blockhash,
        //     false,
        //     compute_unit_price_micro_lamports,
        //     true,
        // )?;
        // let fee = client
        //     .get_fee_for_message(&message)
        //     .await
        //     .map_err(anchor_client::ClientError::from)?;
        let ixs = self.instructions_with_options(true, None);
        let mut compute_budget = self.compute_budget;
        if let Some(price) = compute_unit_price_micro_lamports {
            compute_budget = compute_budget.with_price(price);
        }
        let num_signers = ixs
            .iter()
            .flat_map(|ix| ix.accounts.iter())
            .filter(|meta| meta.is_signer)
            .count() as u64;
        let fee = num_signers * 5_000 + compute_budget.fee();
        Ok(fee)
    }
}
