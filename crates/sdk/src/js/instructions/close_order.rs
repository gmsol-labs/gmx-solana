use std::collections::{HashMap, HashSet};

use crate::{
    builders::{
        order::{CloseOrder, CloseOrderHint, SettleBuilderFee, SettleBuilderFeeHint},
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
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
    payer: StringPubkey,
    orders: HashMap<StringPubkey, CloseOrderHint>,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
    /// Per-order `settle_builder_fee` hints, keyed by order address.
    ///
    /// When present, the keys must match `orders` exactly. Each order's
    /// `settle_builder_fee` is emitted in the same atomic group as its close, ahead of the
    /// close instruction, so the escrow still exists when it runs and neither can land
    /// without the other.
    ///
    /// Each hint must be built from a fresh read of its order account; only field
    /// presence is validated here, never freshness, and a stale hint reverts the close
    /// along with the settle. See `SettleBuilderFeeHint` for which field fails how.
    #[serde(default)]
    settle_builder_fee: Option<HashMap<StringPubkey, SettleBuilderFeeHint>>,
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

    let mut settle_hints = args.settle_builder_fee;

    // The hints must cover exactly the orders being closed. A missing key would silently
    // close an order without settling its fee; an extra key names an order this call does
    // not touch, so the caller is working from a stale view either way.
    if let Some(hints) = settle_hints.as_ref() {
        let orders = args.orders.keys().collect::<HashSet<_>>();
        let settled = hints.keys().collect::<HashSet<_>>();
        if orders != settled {
            let mut missing = orders
                .difference(&settled)
                .map(|key| key.0.to_string())
                .collect::<Vec<_>>();
            let mut unexpected = settled
                .difference(&orders)
                .map(|key| key.0.to_string())
                .collect::<Vec<_>>();
            missing.sort();
            unexpected.sort();
            return Err(crate::Error::custom(format!(
                "`settle_builder_fee` must have the same keys as `orders`: missing {missing:?}, unexpected {unexpected:?}"
            )));
        }
    }

    // Settle and close go in the same atomic group, per order, rather than in two
    // sequential parallel groups. Ordering across parallel groups is a contract of
    // `TransactionGroup` that downstream consumers are not obliged to honour, and if the
    // close landed first the escrow would be gone before the fee was settled. Inside one
    // atomic group the order is the instruction order and the whole thing reverts together.
    let close = args
        .orders
        .into_iter()
        .map(|(order, hint)| {
            let close = CloseOrder::builder()
                .payer(payer)
                .order(order)
                .program(program.clone())
                .build()
                .into_atomic_group(&hint)?;
            match settle_hints.as_mut().and_then(|hints| hints.remove(&order)) {
                Some(settle_hint) => {
                    let mut group = SettleBuilderFee::builder()
                        .payer(payer)
                        .order(order)
                        .program(program.clone())
                        .build()
                        .into_atomic_group(&settle_hint)?;
                    group.merge(close);
                    Ok(group)
                }
                None => Ok(close),
            }
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    TransactionGroup::new(
        group.add(prepare)?.add(close)?.optimize(false),
        &args.recent_blockhash,
        args.compute_unit_price_micro_lamports,
        args.compute_unit_min_priority_lamports,
    )
}
