use std::collections::HashMap;
use wasm_bindgen::prelude::*;

use gmsol_solana_utils::{IntoAtomicGroup, ParallelGroup};

use crate::{
    builders::{order, StoreProgram},
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

#[derive(Debug, serde::Serialize, serde::Deserialize, tsify_next::Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
struct UpdateParams {
    params: order::UpdateOrderParams,
    hint: order::UpdateOrderHint,
}

/// Parameters for updating orders.
#[derive(Debug, serde::Serialize, serde::Deserialize, tsify_next::Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct UpdateOrderArgs {
    recent_blockhash: String,
    payer: StringPubkey,
    orders: HashMap<StringPubkey, UpdateParams>,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build transactions for updating orders.
#[wasm_bindgen]
pub fn update_orders(args: UpdateOrderArgs) -> crate::Result<TransactionGroup> {
    let mut group = args.transaction_group.build();

    let payer = args.payer;

    let updates = args
        .orders
        .into_iter()
        .map(|(order, params)| {
            let ag = order::UpdateOrder::builder()
                .payer(payer)
                .order(order)
                .params(params.params)
                .program(args.program.clone().unwrap_or_default())
                .build()
                .into_atomic_group(&params.hint)?;
            Ok(ag)
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    TransactionGroup::new(group.add(updates)?.optimize(false), &args.recent_blockhash)
}
