use std::collections::HashMap;

use crate::{
    builders::{
        builder_fee::ClaimBuilderFees,
        order::{SettleBuilderFee, SettleBuilderFeeHint},
        user::SetBuilderFeeFactor,
        StoreProgram,
    },
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};
use gmsol_solana_utils::{IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Arguments for standalone `settle_builder_fee` transactions.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SettleBuilderFeeArgs {
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    payer: StringPubkey,
    /// Orders to settle, keyed by order address.
    ///
    /// Each hint must be built from a fresh read of its order account; only field
    /// presence is validated here, never freshness. See `SettleBuilderFeeHint`.
    orders: HashMap<StringPubkey, SettleBuilderFeeHint>,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build transactions for settling builder fees on one or more orders.
#[wasm_bindgen]
pub fn settle_builder_fee(args: SettleBuilderFeeArgs) -> crate::Result<TransactionGroup> {
    let mut group = args.transaction_group.build();
    let payer = args.payer;
    let program = args.program.unwrap_or_default();

    let settle = args
        .orders
        .into_iter()
        .map(|(order, hint)| {
            Ok(SettleBuilderFee::builder()
                .payer(payer)
                .order(order)
                .program(program.clone())
                .build()
                .into_atomic_group(&hint)?)
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    TransactionGroup::new(
        group.add(settle)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}

/// Arguments for a `claim_builder_fees` transaction.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClaimBuilderFeesArgs {
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    /// Builder (payer and fee owner).
    payer: StringPubkey,
    /// The mint the fees are denominated in.
    token_mint: StringPubkey,
    /// Destination token account to receive the claimed balance.
    destination: StringPubkey,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build a transaction for claiming accumulated builder fees for one token mint.
#[wasm_bindgen]
pub fn claim_builder_fees(args: ClaimBuilderFeesArgs) -> crate::Result<TransactionGroup> {
    let mut group = args.transaction_group.build();
    let program = args.program.unwrap_or_default();

    let claim = ClaimBuilderFees::builder()
        .payer(args.payer)
        .token_mint(args.token_mint)
        .destination(args.destination)
        .program(program)
        .build()
        .into_atomic_group(&())?;

    TransactionGroup::new(
        group.add(claim)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}

/// Arguments for a `set_builder_fee_factor` transaction.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SetBuilderFeeFactorArgs {
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    /// Builder (payer and owner of the User Account).
    payer: StringPubkey,
    /// Factor to advertise. Pass `0` to opt out.
    factor: u128,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build a transaction for setting the builder fee factor on the caller's User Account.
#[wasm_bindgen]
pub fn set_builder_fee_factor(args: SetBuilderFeeFactorArgs) -> crate::Result<TransactionGroup> {
    let mut group = args.transaction_group.build();
    let program = args.program.unwrap_or_default();

    let set = SetBuilderFeeFactor::builder()
        .payer(args.payer)
        .factor(args.factor)
        .program(program)
        .build()
        .into_atomic_group(&())?;

    TransactionGroup::new(
        group.add(set)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}
