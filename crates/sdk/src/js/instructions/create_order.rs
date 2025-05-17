use std::collections::{HashMap, HashSet};

use gmsol_solana_utils::{IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};

use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{
        order::{CreateOrder, CreateOrderHint, CreateOrderKind, CreateOrderParams},
        token::{PrepareTokenAccounts, WrapNative},
        user::PrepareUser,
        StoreProgram,
    },
    utils::serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

/// Options for creating orders.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateOrderOptions {
    recent_blockhash: String,
    payer: StringPubkey,
    collateral_or_swap_out_token: StringPubkey,
    hints: HashMap<StringPubkey, CreateOrderHint>,
    #[serde(default)]
    program: Option<StoreProgram>,
    #[serde(default)]
    pay_token: Option<StringPubkey>,
    #[serde(default)]
    receive_token: Option<StringPubkey>,
    #[serde(default)]
    swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    skip_wrap_native_on_pay: Option<bool>,
    #[serde(default)]
    skip_unwrap_native_on_receive: Option<bool>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
}

/// Build transactions for creating orders.
#[wasm_bindgen]
pub fn create_orders(
    kind: CreateOrderKind,
    orders: Vec<CreateOrderParams>,
    options: CreateOrderOptions,
) -> crate::Result<TransactionGroup> {
    let mut group = options.transaction_group.build();

    let prepare_user = PrepareUser::builder()
        .payer(options.payer)
        .build()
        .into_atomic_group(&())?;

    let pay_token = options
        .pay_token
        .unwrap_or(options.collateral_or_swap_out_token);
    let wrap_native = (kind.is_increase() || kind.is_swap())
        && (pay_token.0 == WrapNative::NATIVE_MINT
            && !options.skip_wrap_native_on_pay.unwrap_or_default());

    let mut tokens = HashSet::default();

    if kind.is_decrease() || kind.is_swap() {
        let receive_token = options
            .receive_token
            .unwrap_or(options.collateral_or_swap_out_token);
        tokens.insert(receive_token);
    }

    let hints = &options.hints;
    let create = orders
        .into_iter()
        .map(|params| {
            let market_token = &params.market_token;
            let hint = hints.get(market_token).ok_or_else(|| {
                crate::Error::custom(format!("hint for {} is not provided", market_token.0))
            })?;

            if !kind.is_swap() {
                tokens.insert(hint.long_token);
                tokens.insert(hint.short_token);
            }

            let amount = params.amount;
            let create = CreateOrder::builder()
                .program(options.program.clone().unwrap_or_default())
                .payer(options.payer)
                .kind(kind)
                .collateral_or_swap_out_token(options.collateral_or_swap_out_token)
                .params(params)
                .pay_token(options.pay_token)
                .receive_token(options.receive_token)
                .swap_path(options.swap_path.clone().unwrap_or_default())
                .unwrap_native_on_receive(
                    !options.skip_unwrap_native_on_receive.unwrap_or_default(),
                )
                .build()
                .into_atomic_group(hint)?;

            let ag = if wrap_native {
                let mut wrap = WrapNative::builder()
                    .owner(options.payer)
                    .lamports(amount.try_into().map_err(crate::Error::custom)?)
                    .build()
                    .into_atomic_group(&true)?;
                wrap.merge(create);
                wrap
            } else {
                create
            };

            Ok(ag)
        })
        .collect::<crate::Result<ParallelGroup>>()?;

    let prepare = PrepareTokenAccounts::builder()
        .owner(options.payer)
        .payer(options.payer)
        .tokens(tokens)
        .build()
        .into_atomic_group(&())?;

    TransactionGroup::new(
        group
            .add(prepare_user)?
            .add(prepare)?
            .add(create)?
            .optimize(false),
        &options.recent_blockhash,
    )
}
