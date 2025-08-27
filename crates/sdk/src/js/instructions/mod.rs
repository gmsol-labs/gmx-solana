use std::{collections::HashMap, sync::Arc};

use crate::{
    serde::StringPubkey,
    solana_utils::transaction_group::TransactionGroupOptions as SdkTransactionGroupOptions,
};
use gmsol_solana_utils::{
    instruction_group::ComputeBudgetOptions, signer::TransactionSigners,
    transaction_builder::default_before_sign,
};
use serde::{Deserialize, Serialize};
use solana_sdk::{signature::NullSigner, transaction::VersionedTransaction};
use tsify_next::Tsify;
use wasm_bindgen::prelude::wasm_bindgen;

/// Create order.
pub mod create_order;

/// Close order.
pub mod close_order;

/// Update order.
pub mod update_order;

/// A JS version transaction group options.
#[derive(Debug, Serialize, Deserialize, Tsify, Default)]
#[tsify(from_wasm_abi)]
pub struct TransactionGroupOptions {
    #[serde(default)]
    max_transaction_size: Option<usize>,
    #[serde(default)]
    max_instructions_per_tx: Option<usize>,
    #[serde(default)]
    luts: HashMap<StringPubkey, Vec<StringPubkey>>,
    #[serde(default)]
    memo: Option<String>,
}

impl<'a> From<&'a TransactionGroupOptions> for SdkTransactionGroupOptions {
    fn from(value: &'a TransactionGroupOptions) -> Self {
        let mut options = SdkTransactionGroupOptions::default();
        if let Some(size) = value.max_transaction_size {
            options.max_transaction_size = size;
        }
        if let Some(num) = value.max_instructions_per_tx {
            options.max_instructions_per_tx = num;
        }
        options.memo = value.memo.clone();
        options
    }
}

impl TransactionGroupOptions {
    pub(crate) fn build(&self) -> gmsol_solana_utils::TransactionGroup {
        gmsol_solana_utils::TransactionGroup::with_options_and_luts(
            self.into(),
            self.luts
                .iter()
                .map(|(pubkey, addresses)| {
                    (**pubkey, addresses.iter().map(|pubkey| **pubkey).collect())
                })
                .collect(),
        )
    }
}

/// A JS binding for compiled transaction group.
#[wasm_bindgen]
pub struct TransactionGroup(Vec<Vec<VersionedTransaction>>);

/// Serialized transaction group.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SerializedTransactionGroup(Vec<Vec<Vec<u8>>>);

#[wasm_bindgen]
impl TransactionGroup {
    /// Returns serialized transaciton group.
    pub fn serialize(&self) -> crate::Result<SerializedTransactionGroup> {
        let serialized = self
            .0
            .iter()
            .map(|batch| {
                batch
                    .iter()
                    .map(|txn| Ok(bincode::serialize(txn)?))
                    .collect::<crate::Result<Vec<_>>>()
            })
            .collect::<crate::Result<Vec<_>>>()?;
        Ok(SerializedTransactionGroup(serialized))
    }
}

/// Build transaction options.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct BuildTransactionOptions {
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
}

impl TransactionGroup {
    fn new(
        group: &gmsol_solana_utils::TransactionGroup,
        recent_blockhash: &str,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> crate::Result<Self> {
        let signers = empty_signers();
        let transactions = group
            .to_transactions_with_options(
                &signers,
                recent_blockhash.parse().map_err(crate::Error::custom)?,
                true,
                ComputeBudgetOptions {
                    without_compute_budget: false,
                    compute_unit_price_micro_lamports,
                    compute_unit_min_priority_lamports,
                },
                default_before_sign,
            )
            .map(|res| res.map_err(crate::Error::from))
            .collect::<crate::Result<Vec<_>>>()?;
        Ok(Self(transactions))
    }
}

fn empty_signers() -> TransactionSigners<Arc<NullSigner>> {
    Default::default()
}
