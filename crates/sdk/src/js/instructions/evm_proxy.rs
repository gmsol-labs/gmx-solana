use super::{TransactionGroup, TransactionGroupOptions};
use crate::{
    builders::evm_proxy::Cancel,
    js::evm_proxy::{parse_hex, EvmProxyCancelArgs},
    serde::StringPubkey,
};
use gmsol_evm_proxy_intent::u64_word;
use gmsol_solana_utils::IntoAtomicGroup;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// EVM signatures encode recovery IDs 0 and 1 as `v` 27 and 28.
const EVM_RECOVERY_ID_OFFSET: u8 = 27;

/// Arguments for a signed Cancel transaction.
#[derive(Serialize, Deserialize, Tsify)]
#[tsify(from_wasm_abi)]
pub struct BuildEvmProxyCancelTransactionArgs {
    /// Signed Cancel fields.
    #[serde(flatten)]
    intent: EvmProxyCancelArgs,
    /// 65-byte `r || s || v` signature in hex, as viem `signTypedData` returns it.
    signature: String,
    /// Transaction fee payer and submitter.
    payer: StringPubkey,
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build transactions for a signed Cancel.
#[wasm_bindgen]
pub fn build_evm_proxy_cancel_transaction(
    args: BuildEvmProxyCancelTransactionArgs,
) -> crate::Result<TransactionGroup> {
    let program = args.intent.program();
    let intent = args.intent.intent(&program)?;
    let [signature @ .., v] = parse_hex::<65>(&args.signature, "signature")?;
    let recovery_id = v.checked_sub(EVM_RECOVERY_ID_OFFSET).unwrap_or(v);
    let cancel = Cancel::builder()
        .program(program)
        .payer(args.payer)
        .chain_id(u64_word(args.intent.chain_id))
        .intent(intent)
        .signature(signature)
        .recovery_id(recovery_id)
        .build()
        .into_atomic_group(&())?;
    let mut group = args.transaction_group.build();
    TransactionGroup::new(
        group.add(cancel)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}
