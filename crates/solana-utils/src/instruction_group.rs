use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashSet},
    ops::Deref,
};

use smallvec::SmallVec;
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::NullSigner,
    signer::Signer,
    transaction::VersionedTransaction,
};

use crate::{
    address_lookup_table::AddressLookupTables, compute_budget::ComputeBudget,
    signer::BoxClonableSigner, transaction_group::TransactionGroupOptions,
};

const ATOMIC_SIZE: usize = 3;
const PARALLEL_SIZE: usize = 2;

/// A trait representing types that can be converted into [`AtomicGroup`]s.
pub trait IntoAtomicGroup {
    /// Hint.
    type Hint;

    /// Convert into an [`AtomicGroup`].
    fn into_atomic_group(self, hint: &Self::Hint) -> crate::Result<AtomicGroup>;

    /// Convert into an [`AtomicGroup`] with RPC client.
    #[cfg(client_traits)]
    fn into_atomic_group_with_rpc_client(
        self,
        client: &impl crate::client_traits::RpcClient,
    ) -> impl std::future::Future<Output = crate::Result<AtomicGroup>>
    where
        Self: Sized,
        Self::Hint: crate::client_traits::FromRpcClientWith<Self>,
    {
        use crate::client_traits::FromRpcClientWith;

        async move {
            let hint = Self::Hint::from_rpc_client_with(&self, client).await?;
            self.into_atomic_group(&hint)
        }
    }
}

/// Options for getting instructions.
#[derive(Debug, Clone, Default)]
pub struct GetInstructionsOptions {
    /// Options for compute budget.
    pub compute_budget: ComputeBudgetOptions,
    /// If set, a memo will be included in the final transaction.
    pub memo: Option<String>,
    /// If set, the signer list for the memo instruction will be replaced.
    pub memo_signers: Option<Vec<Pubkey>>,
    /// Extra compute units.
    pub extra_compute_units: u32,
}

/// Options for compute budget.
#[derive(Debug, Clone, Default)]
pub struct ComputeBudgetOptions {
    /// Without compute budget instruction.
    pub without_compute_budget: bool,
    /// Compute unit price in micro lamports.
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Compute unit min priority lamports.
    pub compute_unit_min_priority_lamports: Option<u64>,
}

/// Options type for [`AtomicGroup`].
#[derive(Debug, Clone)]
pub struct AtomicGroupOptions {
    /// Indicates whether the group is mergeable.
    pub is_mergeable: bool,
}

impl Default for AtomicGroupOptions {
    fn default() -> Self {
        Self { is_mergeable: true }
    }
}

/// A group of instructions that are expected to be executed in the same transaction.
#[derive(Debug, Clone)]
pub struct AtomicGroup {
    payer: Pubkey,
    signers: BTreeMap<Pubkey, NullSigner>,
    owned_signers: BTreeMap<Pubkey, BoxClonableSigner<'static>>,
    instructions: SmallVec<[Instruction; ATOMIC_SIZE]>,
    compute_budget: ComputeBudget,
    options: AtomicGroupOptions,
}

impl AtomicGroup {
    /// Returns whether the atomic group is mergeable.
    pub fn is_mergeable(&self) -> bool {
        self.options().is_mergeable
    }

    /// Returns the options of the group.
    pub fn options(&self) -> &AtomicGroupOptions {
        &self.options
    }

    /// Create from an iterator of instructions and options.
    pub fn with_instructions_and_options(
        payer: &Pubkey,
        instructions: impl IntoIterator<Item = Instruction>,
        options: AtomicGroupOptions,
    ) -> Self {
        Self {
            payer: *payer,
            signers: BTreeMap::from([(*payer, NullSigner::new(payer))]),
            owned_signers: Default::default(),
            instructions: SmallVec::from_iter(instructions),
            compute_budget: Default::default(),
            options,
        }
    }

    /// Create from an iterator of instructions.
    pub fn with_instructions(
        payer: &Pubkey,
        instructions: impl IntoIterator<Item = Instruction>,
    ) -> Self {
        Self::with_instructions_and_options(payer, instructions, Default::default())
    }

    /// Create a new empty group.
    pub fn new(payer: &Pubkey) -> Self {
        Self::with_instructions(payer, None)
    }

    /// Add an instruction.
    pub fn add(&mut self, instruction: Instruction) -> &mut Self {
        self.instructions.push(instruction);
        self
    }

    /// Add a signer.
    pub fn add_signer(&mut self, signer: &Pubkey) -> &mut Self {
        self.signers.insert(*signer, NullSigner::new(signer));
        self
    }

    /// Add an owned signer.
    pub fn add_owned_signer(&mut self, signer: impl Signer + Clone + 'static) -> &mut Self {
        self.owned_signers
            .insert(signer.pubkey(), BoxClonableSigner::new(signer));
        self
    }

    /// Get compute budget.
    pub fn compute_budget(&self) -> &ComputeBudget {
        &self.compute_budget
    }

    /// Get mutable reference to the compute budget.
    pub fn compute_budget_mut(&mut self) -> &mut ComputeBudget {
        &mut self.compute_budget
    }

    /// Returns the pubkey of the payer.
    pub fn payer(&self) -> &Pubkey {
        &self.payer
    }

    /// Returns signers that need to be provided externally including the payer.
    pub fn external_signers(&self) -> impl Iterator<Item = &Pubkey> + '_ {
        self.signers.keys()
    }

    fn compute_budget_instructions(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
        extra_compute_units: u32,
    ) -> Vec<Instruction> {
        self.compute_budget
            .compute_budget_instructions_with_extra_units(
                compute_unit_price_micro_lamports,
                compute_unit_min_priority_lamports,
                extra_compute_units,
            )
    }

    /// Returns instructions.
    pub fn instructions_with_options(
        &self,
        options: GetInstructionsOptions,
    ) -> impl Iterator<Item = Cow<'_, Instruction>> {
        let compute_budget_instructions = if options.compute_budget.without_compute_budget {
            Vec::default()
        } else {
            self.compute_budget_instructions(
                options.compute_budget.compute_unit_price_micro_lamports,
                options.compute_budget.compute_unit_min_priority_lamports,
                options.extra_compute_units,
            )
        };
        let memo_signers = match options.memo_signers.as_ref() {
            Some(signers) => signers.iter().collect(),
            None => Vec::from([&self.payer]),
        };
        let memo_instruction = options
            .memo
            .as_ref()
            .map(|s| spl_memo::build_memo(s.as_bytes(), &memo_signers));
        compute_budget_instructions
            .into_iter()
            .chain(memo_instruction)
            .map(Cow::Owned)
            .chain(self.instructions.iter().map(Cow::Borrowed))
    }

    /// Estimates the transaciton size.
    pub fn transaction_size(
        &self,
        is_versioned_transaction: bool,
        luts: Option<&AddressLookupTables>,
        options: GetInstructionsOptions,
    ) -> usize {
        crate::utils::transaction_size_with_luts(
            self.payer,
            &self.instructions_with_options(options).collect::<Vec<_>>(),
            is_versioned_transaction,
            luts,
        )
    }

    /// Estimates the transaction size after merge.
    pub fn transaction_size_after_merge(
        &self,
        other: &Self,
        is_versioned_transaction: bool,
        luts: Option<&AddressLookupTables>,
        options: GetInstructionsOptions,
    ) -> usize {
        crate::utils::transaction_size_with_luts(
            self.payer,
            &self
                .instructions_with_options(options)
                .chain(other.instructions_with_options(GetInstructionsOptions {
                    compute_budget: ComputeBudgetOptions {
                        without_compute_budget: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }))
                .collect::<Vec<_>>(),
            is_versioned_transaction,
            luts,
        )
    }

    /// Merge two [`AtomicGroup`]s.
    ///
    /// # Note
    /// - Merging does not change the payer of the current [`AtomicGroup`].
    pub fn merge(&mut self, mut other: Self) -> &mut Self {
        self.signers.append(&mut other.signers);
        self.owned_signers.append(&mut other.owned_signers);
        self.instructions.extend(other.instructions);
        self.compute_budget += other.compute_budget;
        self
    }

    fn v0_message_with_blockhash_and_options(
        &self,
        recent_blockhash: Hash,
        options: GetInstructionsOptions,
        luts: Option<&AddressLookupTables>,
    ) -> crate::Result<v0::Message> {
        let instructions = self
            .instructions_with_options(options)
            .map(|ix| (*ix).clone())
            .collect::<Vec<_>>();
        let luts = luts
            .map(|t| t.accounts().collect::<Vec<_>>())
            .unwrap_or_default();
        Ok(v0::Message::try_compile(
            self.payer(),
            &instructions,
            &luts,
            recent_blockhash,
        )?)
    }

    /// Create versioned message with the given blockhash and options.
    pub fn message_with_blockhash_and_options(
        &self,
        recent_blockhash: Hash,
        options: GetInstructionsOptions,
        luts: Option<&AddressLookupTables>,
    ) -> crate::Result<VersionedMessage> {
        Ok(VersionedMessage::V0(
            self.v0_message_with_blockhash_and_options(recent_blockhash, options, luts)?,
        ))
    }

    /// Create partially signed transaction with the given blockhash and options.
    pub fn partially_signed_transaction_with_blockhash_and_options(
        &self,
        recent_blockhash: Hash,
        options: GetInstructionsOptions,
        luts: Option<&AddressLookupTables>,
        mut before_sign: impl FnMut(&VersionedMessage) -> crate::Result<()>,
    ) -> crate::Result<VersionedTransaction> {
        let mut memo_signers = vec![];
        if let Some(signers) = options.memo_signers.as_ref() {
            let signers: BTreeSet<_> = signers.iter().collect();
            for signer in signers {
                if !self.signers.contains_key(signer) && !self.owned_signers.contains_key(signer) {
                    memo_signers.push(NullSigner::new(signer));
                }
            }
        }
        let message = self.message_with_blockhash_and_options(recent_blockhash, options, luts)?;
        (before_sign)(&message)?;
        let signers = self
            .signers
            .values()
            .chain(memo_signers.iter())
            .map(|s| s as &dyn Signer)
            .chain(self.owned_signers.values().map(|s| s as &dyn Signer))
            .collect::<Vec<_>>();
        Ok(VersionedTransaction::try_new(message, &signers)?)
    }

    /// Estimates the execution fee of the result transaction.
    pub fn estimate_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        self.estimate_execution_fee_with_extra_units(
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
            0,
        )
    }

    /// Estimates the execution fee of the result transaction with extra compute units.
    pub fn estimate_execution_fee_with_extra_units(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
        extra_compute_units: u32,
    ) -> u64 {
        let ixs = self
            .instructions_with_options(GetInstructionsOptions {
                compute_budget: ComputeBudgetOptions {
                    without_compute_budget: true,
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let num_signers = ixs
            .iter()
            .flat_map(|ix| ix.accounts.iter())
            .filter(|meta| meta.is_signer)
            .map(|meta| &meta.pubkey)
            .collect::<HashSet<_>>()
            .len() as u64;
        num_signers * 5_000
            + self.compute_budget.fee_with_extra_units(
                compute_unit_price_micro_lamports,
                compute_unit_min_priority_lamports,
                extra_compute_units,
            )
    }
}

impl Extend<Instruction> for AtomicGroup {
    fn extend<T: IntoIterator<Item = Instruction>>(&mut self, iter: T) {
        self.instructions.extend(iter);
    }
}

impl Deref for AtomicGroup {
    type Target = [Instruction];

    fn deref(&self) -> &Self::Target {
        self.instructions.deref()
    }
}

/// The options type for [`ParallelGroup`].
#[derive(Debug, Clone)]
pub struct ParallelGroupOptions {
    /// Indicates whether the [`ParallelGroup`] is mergeable.
    pub is_mergeable: bool,
}

impl Default for ParallelGroupOptions {
    fn default() -> Self {
        Self { is_mergeable: true }
    }
}

/// A group of atomic instructions that can be executed in parallel.
#[derive(Debug, Clone, Default)]
pub struct ParallelGroup {
    groups: SmallVec<[AtomicGroup; PARALLEL_SIZE]>,
    options: ParallelGroupOptions,
}

impl ParallelGroup {
    /// Create a new [`ParallelGroup`] with the given options.
    pub fn with_options(
        groups: impl IntoIterator<Item = AtomicGroup>,
        options: ParallelGroupOptions,
    ) -> Self {
        Self {
            groups: FromIterator::from_iter(groups),
            options,
        }
    }

    /// Returns the options.
    pub fn options(&self) -> &ParallelGroupOptions {
        &self.options
    }

    /// Returns whether the group is mergeable.
    pub fn is_mergeable(&self) -> bool {
        self.options().is_mergeable
    }

    /// Set whether the group is mergeable.
    pub fn set_is_mergeable(&mut self, is_mergeable: bool) -> &mut Self {
        self.options.is_mergeable = is_mergeable;
        self
    }

    /// Add an [`AtomicGroup`].
    pub fn add(&mut self, group: AtomicGroup) -> &mut Self {
        self.groups.push(group);
        self
    }

    pub(crate) fn optimize(
        &mut self,
        options: &TransactionGroupOptions,
        luts: &AddressLookupTables,
        allow_payer_change: bool,
    ) -> &mut Self {
        if options.optimize(&mut self.groups, luts, allow_payer_change) {
            self.groups = self
                .groups
                .drain(..)
                .filter(|group| !group.is_empty())
                .collect();
        }
        self
    }

    pub(crate) fn single(&self) -> Option<&AtomicGroup> {
        if self.groups.len() == 1 {
            Some(&self.groups[0])
        } else {
            None
        }
    }

    pub(crate) fn single_mut(&mut self) -> Option<&mut AtomicGroup> {
        if self.groups.len() == 1 {
            Some(&mut self.groups[0])
        } else {
            None
        }
    }

    pub(crate) fn into_single(mut self) -> Option<AtomicGroup> {
        if self.groups.len() == 1 {
            Some(self.groups.remove(0))
        } else {
            None
        }
    }

    /// Returns the total number of transactions.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Returns whether the group is empty.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Estiamtes the execution fee of the result transactions
    pub fn estimate_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        self.estimate_execution_fee_with_extra_units(
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
            0,
        )
    }

    /// Estiamtes the execution fee of the result transactions with extra units.
    pub fn estimate_execution_fee_with_extra_units(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
        extra_compute_units: u32,
    ) -> u64 {
        self.groups
            .iter()
            .map(|ag| {
                ag.estimate_execution_fee_with_extra_units(
                    compute_unit_price_micro_lamports,
                    compute_unit_min_priority_lamports,
                    extra_compute_units,
                )
            })
            .sum()
    }
}

impl From<AtomicGroup> for ParallelGroup {
    fn from(value: AtomicGroup) -> Self {
        let mut this = Self::default();
        this.add(value);
        this
    }
}

impl FromIterator<AtomicGroup> for ParallelGroup {
    fn from_iter<T: IntoIterator<Item = AtomicGroup>>(iter: T) -> Self {
        Self::with_options(iter, Default::default())
    }
}

impl Deref for ParallelGroup {
    type Target = [AtomicGroup];

    fn deref(&self) -> &Self::Target {
        self.groups.deref()
    }
}
