use std::collections::{hash_map, HashMap, HashSet};

use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};

use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{
        callback::Callback,
        order::{
            CreateOrder, CreateOrderHint, CreateOrderKind, CreateOrderParams, PreparePosition,
        },
        token::{PrepareTokenAccounts, WrapNative},
        user::PrepareUser,
        StoreProgram,
    },
    js::instructions::BuildTransactionOptions,
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

/// Options for creating orders.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateOrderOptions {
    recent_blockhash: String,
    #[serde(default)]
    compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    compute_unit_min_priority_lamports: Option<u64>,
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
    callback: Option<Callback>,
    #[serde(default)]
    transaction_group: TransactionGroupOptions,
    #[serde(default)]
    force_create_positions_in_parallel: Option<bool>,
    #[serde(default)]
    force_create_positions: Option<bool>,
}

/// Create transaction builder for create-order ixs.
#[wasm_bindgen]
pub fn create_orders_builder(
    kind: CreateOrderKind,
    orders: Vec<CreateOrderParams>,
    options: CreateOrderOptions,
) -> crate::Result<CreateOrdersBuilder> {
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

    if wrap_native {
        tokens.insert(WrapNative::NATIVE_MINT.into());
    }

    let hints = &options.hints;
    let force_create_positions_in_parallel = options
        .force_create_positions_in_parallel
        .unwrap_or_default();
    let force_create_positions =
        options.force_create_positions.unwrap_or_default() || force_create_positions_in_parallel;

    let mut positions = HashMap::<StringPubkey, _>::default();
    let create = orders
        .into_iter()
        .map(|params| {
            let market_token = &params.market_token;
            let hint = hints.get(market_token).ok_or_else(|| {
                crate::Error::custom(format!("hint for {} is not provided", market_token.0))
            })?;

            let program = options.program.clone().unwrap_or_default();
            let payer = options.payer;
            let collateral_or_swap_out_token = options.collateral_or_swap_out_token;
            if !kind.is_swap() {
                tokens.insert(hint.long_token);
                tokens.insert(hint.short_token);

                if force_create_positions && !force_create_positions_in_parallel {
                    let prepare = PreparePosition::builder()
                        .program(program.clone())
                        .collateral_token(collateral_or_swap_out_token)
                        .kind(kind)
                        .params(params.clone())
                        .payer(payer)
                        .build();

                    if let hash_map::Entry::Vacant(e) =
                        positions.entry(prepare.position_address().into())
                    {
                        let ag = prepare.into_atomic_group(hint)?;
                        e.insert(ag);
                    }
                }
            }

            let amount = params.amount;
            let create = CreateOrder::builder()
                .program(program)
                .payer(payer)
                .kind(kind)
                .collateral_or_swap_out_token(collateral_or_swap_out_token)
                .params(params)
                .pay_token(options.pay_token)
                .receive_token(options.receive_token)
                .swap_path(options.swap_path.clone().unwrap_or_default())
                .unwrap_native_on_receive(
                    !options.skip_unwrap_native_on_receive.unwrap_or_default(),
                )
                .callback(options.callback.clone())
                .skip_position_creation(
                    force_create_positions && !force_create_positions_in_parallel,
                )
                .force_position_creation(force_create_positions_in_parallel)
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
        .collect::<crate::Result<Vec<_>>>()?;

    Ok(CreateOrdersBuilder {
        payer: options.payer,
        tokens,
        positions,
        create,
        transaction_group: options.transaction_group,
        build: BuildTransactionOptions {
            recent_blockhash: options.recent_blockhash,
            compute_unit_price_micro_lamports: options.compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports: options.compute_unit_min_priority_lamports,
        },
    })
}

/// Builder for create-order ixs.
#[wasm_bindgen]
pub struct CreateOrdersBuilder {
    payer: StringPubkey,
    tokens: HashSet<StringPubkey>,
    positions: HashMap<StringPubkey, AtomicGroup>,
    create: Vec<AtomicGroup>,
    transaction_group: TransactionGroupOptions,
    build: BuildTransactionOptions,
}

#[wasm_bindgen]
impl CreateOrdersBuilder {
    /// Build transactions.
    pub fn build_with_options(
        self,
        transaction_group: Option<TransactionGroupOptions>,
        build: Option<BuildTransactionOptions>,
    ) -> crate::Result<TransactionGroup> {
        let mut group = transaction_group.unwrap_or(self.transaction_group).build();

        let prepare_user = PrepareUser::builder()
            .payer(self.payer)
            .build()
            .into_atomic_group(&())?;

        let prepare = PrepareTokenAccounts::builder()
            .owner(self.payer)
            .payer(self.payer)
            .tokens(self.tokens)
            .build()
            .into_atomic_group(&())?;

        let build = build.unwrap_or(self.build);
        TransactionGroup::new(
            group
                .add(prepare_user)?
                .add(prepare)?
                .add(self.positions.into_values().collect::<ParallelGroup>())?
                .add(self.create.into_iter().collect::<ParallelGroup>())?
                .optimize(false),
            &build.recent_blockhash,
            build.compute_unit_price_micro_lamports,
            build.compute_unit_min_priority_lamports,
        )
    }

    /// Merge with the other [`CreateOrderBuilder`].
    pub fn merge(&mut self, other: &mut Self) -> crate::Result<()> {
        if self.payer != other.payer {
            return Err(crate::Error::custom(format!(
                "payer mismatch: this = {}, other = {}",
                self.payer, other.payer
            )));
        }
        for token in other.tokens.iter() {
            self.tokens.insert(*token);
        }
        for (position, ag) in other.positions.drain() {
            self.positions.entry(position).or_insert(ag);
        }
        self.create.append(&mut other.create);
        Ok(())
    }
}

/// Build transactions for creating orders.
#[wasm_bindgen]
pub fn create_orders(
    kind: CreateOrderKind,
    orders: Vec<CreateOrderParams>,
    options: CreateOrderOptions,
) -> crate::Result<TransactionGroup> {
    create_orders_builder(kind, orders, options)?.build_with_options(None, None)
}
