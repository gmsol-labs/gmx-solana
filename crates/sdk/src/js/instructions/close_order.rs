use std::collections::{HashMap, HashSet};

use crate::{
    builders::{
        order::{CloseOrder, CloseOrderHint},
        token::PrepareTokenAccounts,
        StoreProgram,
    },
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};
use gmsol_solana_utils::{IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Parameters for closing orders.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CloseOrderArgs {
    recent_blockhash: String,
    payer: StringPubkey,
    orders: HashMap<StringPubkey, CloseOrderHint>,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build transactions for closing orders.
#[wasm_bindgen]
pub fn close_orders(args: CloseOrderArgs) -> crate::Result<TransactionGroup> {
    let mut group = args.transaction_group.build();

    let payer = args.payer;
    let program = args.program.unwrap_or_default();
    let mut tokens = HashMap::<_, HashSet<_>>::default();

    for hint in args.orders.values() {
        let owner = hint.owner;
        let owner_tokens = tokens.entry(owner).or_default();
        if let Some(token) = hint.initial_collateral_token {
            owner_tokens.insert(token);
        }

        let receiver = hint.receiver;
        let receiver_tokens = tokens.entry(receiver).or_default();
        if let Some(token) = hint.final_output_token {
            receiver_tokens.insert(token);
        }
        if let Some(token) = hint.long_token {
            receiver_tokens.insert(token);
        }
        if let Some(token) = hint.short_token {
            receiver_tokens.insert(token);
        }
    }

    let prepare = tokens
        .into_iter()
        .map(|(owner, tokens)| {
            Ok(PrepareTokenAccounts::builder()
                .owner(owner)
                .payer(payer)
                .tokens(tokens)
                .build()
                .into_atomic_group(&())?)
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    let close = args
        .orders
        .into_iter()
        .map(|(order, hint)| {
            Ok(CloseOrder::builder()
                .payer(payer)
                .order(order)
                .program(program.clone())
                .build()
                .into_atomic_group(&hint)?)
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    TransactionGroup::new(
        group.add(prepare)?.add(close)?.optimize(false),
        &args.recent_blockhash,
    )
}
