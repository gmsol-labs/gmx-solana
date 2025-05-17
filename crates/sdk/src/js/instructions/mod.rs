use std::{collections::HashMap, sync::Arc};

use crate::{
    solana_utils::transaction_group::TransactionGroupOptions as SdkTransactionGroupOptions,
    utils::serde::StringPubkey,
};
use gmsol_solana_utils::signer::TransactionSigners;
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
    compute_unit_price_micro_lamports: Option<u64>,
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
        options.compute_unit_price_micro_lamports = value.compute_unit_price_micro_lamports;
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

/// A JS binding for transaction group.
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

impl TransactionGroup {
    fn new(
        group: &gmsol_solana_utils::TransactionGroup,
        recent_blockhash: &str,
    ) -> crate::Result<Self> {
        let signers = empty_signers();
        let transactions = group
            .to_transactions(
                &signers,
                recent_blockhash.parse().map_err(crate::Error::custom)?,
                true,
            )
            .map(|res| res.map_err(crate::Error::from))
            .collect::<crate::Result<Vec<_>>>()?;
        Ok(Self(transactions))
    }
}

fn empty_signers() -> TransactionSigners<Arc<NullSigner>> {
    Default::default()
}
