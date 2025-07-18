use std::{borrow::BorrowMut, ops::Deref};

use solana_sdk::{
    hash::Hash, message::VersionedMessage, packet::PACKET_DATA_SIZE, pubkey::Pubkey,
    signer::Signer, transaction::VersionedTransaction,
};

use crate::{
    address_lookup_table::AddressLookupTables,
    instruction_group::{ComputeBudgetOptions, GetInstructionsOptions},
    signer::TransactionSigners,
    transaction_builder::default_before_sign,
    AtomicGroup, ParallelGroup,
};

/// Transaction Group Options.
#[derive(Debug, Clone)]
pub struct TransactionGroupOptions {
    /// Max transaction size.
    pub max_transaction_size: usize,
    /// Max instructions per transaction.
    /// # Note
    /// - Compute budget instructions are ignored.
    pub max_instructions_per_tx: usize,
    // /// Compute unit price in micro lamports.
    // pub compute_unit_price_micro_lamports: Option<u64>,
    /// Memo for each transaction in this group.
    pub memo: Option<String>,
}

impl Default for TransactionGroupOptions {
    fn default() -> Self {
        Self {
            max_transaction_size: PACKET_DATA_SIZE,
            max_instructions_per_tx: 14,
            // compute_unit_price_micro_lamports: None,
            memo: None,
        }
    }
}

impl TransactionGroupOptions {
    fn instruction_options(&self, compute_budget: &ComputeBudgetOptions) -> GetInstructionsOptions {
        GetInstructionsOptions {
            compute_budget: compute_budget.clone(),
            memo: self.memo.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_transaction_batch<C: Deref<Target = impl Signer + ?Sized>>(
        &self,
        recent_blockhash: Hash,
        luts: &AddressLookupTables,
        compute_budget: &ComputeBudgetOptions,
        group: &ParallelGroup,
        signers: &TransactionSigners<C>,
        allow_partial_sign: bool,
        mut before_sign: impl FnMut(&VersionedMessage) -> crate::Result<()>,
    ) -> crate::Result<Vec<VersionedTransaction>> {
        group
            .iter()
            .map(|ag| {
                signers.sign_atomic_instruction_group(
                    ag,
                    recent_blockhash,
                    self.instruction_options(compute_budget),
                    Some(luts),
                    allow_partial_sign,
                    &mut before_sign,
                )
            })
            .collect()
    }

    fn optimizable(
        &self,
        x: &AtomicGroup,
        y: &AtomicGroup,
        luts: &AddressLookupTables,
        allow_payer_change: bool,
    ) -> bool {
        if !x.is_mergeable() || !y.is_mergeable() {
            return false;
        }

        if !allow_payer_change && x.payer() != y.payer() {
            return false;
        }

        let num_ixs = x.len() + y.len();
        if num_ixs > self.max_instructions_per_tx {
            return false;
        }

        let size = x.transaction_size_after_merge(y, true, Some(luts), Default::default());
        if size > self.max_transaction_size {
            return false;
        }

        true
    }

    pub(crate) fn optimize<T: BorrowMut<AtomicGroup>>(
        &self,
        groups: &mut [T],
        luts: &AddressLookupTables,
        allow_payer_change: bool,
    ) -> bool {
        let indices = (0..groups.len()).collect::<Vec<_>>();

        let mut merged = false;
        let default_pubkey = Pubkey::default();
        for pair in indices.windows(2) {
            let [i, j] = *pair else { unreachable!() };
            if groups[i].borrow().is_empty() {
                // If the current group is empty, it can be considered as already merged into the following group.
                merged = true;
                continue;
            }
            if !self.optimizable(
                groups[i].borrow(),
                groups[j].borrow(),
                luts,
                allow_payer_change,
            ) {
                continue;
            }
            let mut group = AtomicGroup::new(&default_pubkey);
            std::mem::swap(groups[i].borrow_mut(), &mut group);
            std::mem::swap(groups[j].borrow_mut(), &mut group);
            groups[j].borrow_mut().merge(group);
            merged = true;
        }

        merged
    }
}

/// Transaction Group.
#[derive(Debug, Clone, Default)]
pub struct TransactionGroup {
    options: TransactionGroupOptions,
    luts: AddressLookupTables,
    groups: Vec<ParallelGroup>,
}

impl TransactionGroup {
    /// Create with the given [`TransactionGroupOptions`] and [`AddressLookupTables`].
    pub fn with_options_and_luts(
        options: TransactionGroupOptions,
        luts: AddressLookupTables,
    ) -> Self {
        Self {
            options,
            luts,
            groups: Default::default(),
        }
    }

    fn validate_one(&self, group: &AtomicGroup) -> crate::Result<()> {
        if group.len() > self.options.max_instructions_per_tx {
            return Err(crate::Error::AddTransaction(
                "Too many instructions for a signle transaction",
            ));
        }
        let size = group.transaction_size(true, Some(&self.luts), Default::default());
        if size > self.options.max_transaction_size {
            return Err(crate::Error::AddTransaction(
                "Transaction size exceeds the `max_transaction_size` config",
            ));
        }
        Ok(())
    }

    /// Returns [`Ok`] if the given [`ParallelGroup`] can be added without error.
    pub fn validate_instruction_group(&self, group: &ParallelGroup) -> crate::Result<()> {
        for insts in group.iter() {
            self.validate_one(insts)?;
        }
        Ok(())
    }

    /// Add a [`ParallelGroup`].
    pub fn add(&mut self, group: impl Into<ParallelGroup>) -> crate::Result<&mut Self> {
        let group = group.into();
        if group.is_empty() {
            return Ok(self);
        }
        self.validate_instruction_group(&group)?;
        self.groups.push(group);
        Ok(self)
    }

    /// Optimize the transactions by repacking instructions to maximize space efficiency.
    pub fn optimize(&mut self, allow_payer_change: bool) -> &mut Self {
        for group in self.groups.iter_mut() {
            group.optimize(&self.options, &self.luts, allow_payer_change);
        }

        let indices = (0..self.groups.len()).collect::<Vec<_>>();
        let groups = &mut self.groups;

        let mut merged = false;
        for pair in indices.windows(2) {
            let [i, j] = *pair else {
                unreachable!();
            };
            let pg_i = &groups[i];
            let pg_j = &groups[j];

            if !pg_i.is_mergeable() || !pg_j.is_mergeable() {
                continue;
            }

            let (Some(group_i), Some(group_j)) = (pg_i.single(), pg_j.single()) else {
                continue;
            };
            if !self
                .options
                .optimizable(group_i, group_j, &self.luts, allow_payer_change)
            {
                continue;
            }
            let mut group = std::mem::take(&mut groups[i]);
            std::mem::swap(&mut groups[j], &mut group);
            groups[j]
                .single_mut()
                .unwrap()
                .merge(group.into_single().unwrap());
            merged = true;
        }

        if merged {
            self.groups = self
                .groups
                .drain(..)
                .filter(|group| !group.is_empty())
                .collect();
        }

        self
    }

    /// Build transactions.
    pub fn to_transactions<'a, C: Deref<Target = impl Signer + ?Sized>>(
        &'a self,
        signers: &'a TransactionSigners<C>,
        recent_blockhash: Hash,
        allow_partial_sign: bool,
    ) -> TransactionGroupIter<'a, C, fn(&VersionedMessage) -> crate::Result<()>> {
        self.to_transactions_with_options(
            signers,
            recent_blockhash,
            allow_partial_sign,
            Default::default(),
            default_before_sign,
        )
    }

    /// Build transactions.
    pub fn to_transactions_with_options<'a, C: Deref<Target = impl Signer + ?Sized>, F>(
        &'a self,
        signers: &'a TransactionSigners<C>,
        recent_blockhash: Hash,
        allow_partial_sign: bool,
        compute_budget: ComputeBudgetOptions,
        before_sign: F,
    ) -> TransactionGroupIter<'a, C, F>
    where
        F: FnMut(&VersionedMessage) -> crate::Result<()>,
    {
        TransactionGroupIter {
            signers,
            recent_blockhash,
            compute_budget,
            options: &self.options,
            luts: &self.luts,
            iter: self.groups.iter(),
            allow_partial_sign,
            before_sign,
        }
    }

    /// Returns whether the transaction group is empty.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Returns the total number of transactions.
    pub fn len(&self) -> usize {
        self.groups.iter().map(|pg| pg.len()).sum()
    }

    /// Returns the options.
    pub fn options(&self) -> &TransactionGroupOptions {
        &self.options
    }

    /// Estimates the execution fee of the result transaction.
    pub fn estimate_execution_fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        self.groups
            .iter()
            .map(|pg| {
                pg.estimate_execution_fee(
                    compute_unit_price_micro_lamports,
                    compute_unit_min_priority_lamports,
                )
            })
            .sum()
    }

    /// Returns [`ParallelGroup`]s.
    pub fn groups(&self) -> &[ParallelGroup] {
        &self.groups
    }

    /// Returns Address Lookup Tables.
    pub fn luts(&self) -> &AddressLookupTables {
        &self.luts
    }
}

/// Transaction Group Iter.
pub struct TransactionGroupIter<'a, C, F> {
    signers: &'a TransactionSigners<C>,
    recent_blockhash: Hash,
    compute_budget: ComputeBudgetOptions,
    options: &'a TransactionGroupOptions,
    luts: &'a AddressLookupTables,
    iter: std::slice::Iter<'a, ParallelGroup>,
    allow_partial_sign: bool,
    before_sign: F,
}

impl<C: Deref<Target = impl Signer + ?Sized>, F> Iterator for TransactionGroupIter<'_, C, F>
where
    F: FnMut(&VersionedMessage) -> crate::Result<()>,
{
    type Item = crate::Result<Vec<VersionedTransaction>>;

    fn next(&mut self) -> Option<Self::Item> {
        let group = self.iter.next()?;
        Some(self.options.build_transaction_batch(
            self.recent_blockhash,
            self.luts,
            &self.compute_budget,
            group,
            self.signers,
            self.allow_partial_sign,
            &mut self.before_sign,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signature},
    };

    use super::*;

    #[test]
    fn fully_sign() -> crate::Result<()> {
        use solana_sdk::system_instruction::transfer;

        let payer_1 = Arc::new(Keypair::new());
        let payer_1_pubkey = payer_1.pubkey();

        let payer_2 = Arc::new(Keypair::new());
        let payer_2_pubkey = payer_2.pubkey();

        let payer_3 = Arc::new(Keypair::new());
        let payer_3_pubkey = payer_3.pubkey();

        let signers = TransactionSigners::from_iter([payer_1, payer_2, payer_3]);

        let ig = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_2_pubkey);
                ag
            },
            AtomicGroup::with_instructions(
                &payer_3_pubkey,
                [
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                ],
            ),
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let mut group = TransactionGroup::default();
        let txns = group
            .add(ig)?
            .to_transactions(&signers, Hash::default(), false);

        for (idx, res) in txns.enumerate() {
            for txn in res.inspect_err(|err| eprintln!("[{idx}]: {err}"))? {
                txn.verify_and_hash_message()
                    .expect("should be fully signed");
            }
        }
        Ok(())
    }

    #[test]
    fn partially_sign() -> crate::Result<()> {
        use solana_sdk::system_instruction::transfer;

        let payer_1 = Arc::new(Keypair::new());
        let payer_1_pubkey = payer_1.pubkey();

        let payer_2 = Arc::new(Keypair::new());
        let payer_2_pubkey = payer_2.pubkey();

        let payer_3 = Arc::new(Keypair::new());
        let payer_3_pubkey = payer_3.pubkey();

        let signers = TransactionSigners::from_iter([payer_1, payer_3]);

        let ig = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_2_pubkey);
                ag
            },
            AtomicGroup::with_instructions(
                &payer_3_pubkey,
                [
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                ],
            ),
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let mut group = TransactionGroup::default();
        let txns = group
            .add(ig)?
            .to_transactions(&signers, Hash::default(), true);

        for res in txns {
            for txn in res? {
                let results = txn.verify_with_results();
                for (idx, result) in results.into_iter().enumerate() {
                    if !result {
                        assert_eq!(txn.signatures[idx], Signature::default());
                    }
                }
            }
        }

        Ok(())
    }

    #[test]
    fn optimize() -> crate::Result<()> {
        use solana_sdk::system_instruction::transfer;

        let payer_1 = Arc::new(Keypair::new());
        let payer_1_pubkey = payer_1.pubkey();

        let payer_2 = Arc::new(Keypair::new());
        let payer_2_pubkey = payer_2.pubkey();

        let payer_3 = Arc::new(Keypair::new());
        let payer_3_pubkey = payer_3.pubkey();

        let signers = TransactionSigners::from_iter([payer_1, payer_2, payer_3]);

        let ig_1 = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_2_pubkey);
                ag
            },
            AtomicGroup::with_instructions(
                &payer_3_pubkey,
                [
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                ],
            ),
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let ig_2 = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_2_pubkey);
                ag
            },
            AtomicGroup::with_instructions(
                &payer_3_pubkey,
                [
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                ],
            ),
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let mut group = TransactionGroup::default();
        let txns = group
            .add(ig_1)?
            .add(ig_2)?
            .optimize(true)
            .to_transactions(&signers, Hash::default(), false)
            .flat_map(|res| match res {
                Ok(txns) => txns.into_iter().map(Ok).collect(),
                Err(err) => vec![Err(err)],
            })
            .collect::<crate::Result<Vec<_>>>()?;
        assert_eq!(txns.len(), 1);
        assert!(bincode::serialize(&txns[0]).unwrap().len() <= PACKET_DATA_SIZE);
        txns[0]
            .verify_and_hash_message()
            .expect("should be fully signed");
        Ok(())
    }

    #[test]
    fn optimize_deny_payer_change() -> crate::Result<()> {
        use solana_sdk::system_instruction::transfer;

        let payer_1 = Arc::new(Keypair::new());
        let payer_1_pubkey = payer_1.pubkey();

        let payer_2 = Arc::new(Keypair::new());
        let payer_2_pubkey = payer_2.pubkey();

        let payer_3 = Arc::new(Keypair::new());
        let payer_3_pubkey = payer_3.pubkey();

        let signers = TransactionSigners::from_iter([payer_1, payer_2, payer_3]);

        let ig_1 = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_2_pubkey);
                ag
            },
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_1_pubkey,
                    [
                        transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    ],
                );
                ag.add_signer(&payer_3_pubkey);
                ag
            },
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let ig_2 = [
            {
                let mut ag = AtomicGroup::with_instructions(
                    &payer_3_pubkey,
                    [
                        transfer(&payer_1_pubkey, &Pubkey::new_unique(), 1),
                        transfer(&payer_2_pubkey, &payer_1_pubkey, 1),
                    ],
                );
                ag.add_signer(&payer_1_pubkey).add_signer(&payer_2_pubkey);
                ag
            },
            AtomicGroup::with_instructions(
                &payer_3_pubkey,
                [
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                    transfer(&payer_3_pubkey, &Pubkey::new_unique(), 1),
                ],
            ),
        ]
        .into_iter()
        .collect::<ParallelGroup>();

        let mut group = TransactionGroup::default();
        let txns = group
            .add(ig_1)?
            .add(ig_2)?
            .optimize(false)
            .to_transactions(&signers, Hash::default(), false)
            .flat_map(|res| match res {
                Ok(txns) => txns.into_iter().map(Ok).collect(),
                Err(err) => vec![Err(err)],
            })
            .collect::<crate::Result<Vec<_>>>()?;
        assert_eq!(txns.len(), 2);

        for txn in txns {
            assert!(bincode::serialize(&txn).unwrap().len() <= PACKET_DATA_SIZE);
            txn.verify_and_hash_message()
                .expect("should be fully signed");
        }

        Ok(())
    }
}
