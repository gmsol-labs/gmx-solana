use std::collections::{HashMap, HashSet};

use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{
        deposit::{CreateDeposit, CreateDepositHint},
        token::{PrepareTokenAccounts, WrapNative},
        user::PrepareUser,
        StoreProgram,
    },
    js::instructions::BuildTransactionOptions,
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateDepositParamsJs {
    pub market_token: StringPubkey,
    #[serde(default)]
    pub receiver: Option<StringPubkey>,
    #[serde(default)]
    pub long_pay_token: Option<StringPubkey>,
    #[serde(default)]
    pub short_pay_token: Option<StringPubkey>,
    #[serde(default)]
    pub long_swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub short_swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub long_pay_amount: Option<u128>,
    #[serde(default)]
    pub short_pay_amount: Option<u128>,
    #[serde(default)]
    pub min_receive_amount: Option<u128>,
    #[serde(default)]
    pub skip_unwrap_native_on_receive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateDepositOptions {
    pub recent_blockhash: String,
    pub payer: StringPubkey,
    #[serde(default)]
    pub program: Option<StoreProgram>,
    #[serde(default)]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    pub compute_unit_min_priority_lamports: Option<u64>,
    pub hints: HashMap<StringPubkey, CreateDepositHint>,
    #[serde(default)]
    pub transaction_group: TransactionGroupOptions,
    #[serde(default)]
    skip_wrap_native_on_pay: Option<bool>,
}

#[wasm_bindgen]
pub struct CreateDepositsBuilder {
    payer: StringPubkey,
    tokens: HashSet<StringPubkey>,
    groups: Vec<AtomicGroup>,
    transaction_group: TransactionGroupOptions,
    build: BuildTransactionOptions,
}

#[wasm_bindgen]
pub fn create_deposits_builder(
    deposits: Vec<CreateDepositParamsJs>,
    options: CreateDepositOptions,
) -> crate::Result<CreateDepositsBuilder> {
    let mut tokens = HashSet::default();
    let mut groups: Vec<AtomicGroup> = Vec::with_capacity(deposits.len());

    let should_wrap_native = !options.skip_wrap_native_on_pay.unwrap_or_default();
    let mut wrap_native = false;

    for params in deposits.into_iter() {
        let market_token = params.market_token;
        let hint = options.hints.get(&market_token).ok_or_else(|| {
            crate::Error::custom(format!("hint for {} is not provided", market_token.0))
        })?;

        let mut wrap_amount = 0;

        if let Some(t) = params.long_pay_token.as_ref() {
            if should_wrap_native && **t == WrapNative::NATIVE_MINT {
                wrap_amount += params.long_pay_amount.unwrap_or_default();
            }
        }
        if let Some(t) = params.short_pay_token.as_ref() {
            if should_wrap_native && **t == WrapNative::NATIVE_MINT {
                wrap_amount += params.short_pay_amount.unwrap_or_default();
            }
        }
        tokens.insert(market_token);

        let program = options.program.clone().unwrap_or_default();
        let builder = CreateDeposit::builder()
            .program(program)
            .payer(options.payer)
            .market_token(market_token)
            .long_pay_token(params.long_pay_token)
            .short_pay_token(params.short_pay_token)
            .long_swap_path(params.long_swap_path.unwrap_or_default())
            .short_swap_path(params.short_swap_path.unwrap_or_default())
            .long_pay_amount(params.long_pay_amount.unwrap_or_default().try_into()?)
            .short_pay_amount(params.short_pay_amount.unwrap_or_default().try_into()?)
            .min_receive_amount(params.min_receive_amount.unwrap_or_default().try_into()?)
            .unwrap_native_on_receive(!params.skip_unwrap_native_on_receive.unwrap_or_default());

        let built = if let Some(r) = params.receiver {
            builder.receiver(r).build()
        } else {
            builder.build()
        }
        .into_atomic_group(hint)?;

        let ag = if wrap_amount == 0 {
            built
        } else {
            wrap_native = true;
            let mut wrap = WrapNative::builder()
                .owner(options.payer)
                .lamports(wrap_amount.try_into().map_err(crate::Error::custom)?)
                .build()
                .into_atomic_group(&true)?;
            wrap.merge(built);
            wrap
        };

        groups.push(ag);
    }

    if wrap_native {
        tokens.insert(WrapNative::NATIVE_MINT.into());
    }

    Ok(CreateDepositsBuilder {
        payer: options.payer,
        tokens,
        groups,
        transaction_group: options.transaction_group,
        build: BuildTransactionOptions {
            recent_blockhash: options.recent_blockhash,
            compute_unit_price_micro_lamports: options.compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports: options.compute_unit_min_priority_lamports,
        },
    })
}

#[wasm_bindgen]
impl CreateDepositsBuilder {
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

        let prepare_tokens = PrepareTokenAccounts::builder()
            .owner(self.payer)
            .payer(self.payer)
            .tokens(self.tokens)
            .build()
            .into_atomic_group(&())?;

        let build = build.unwrap_or(self.build);
        TransactionGroup::new(
            group
                .add(prepare_user)?
                .add(prepare_tokens)?
                .add(self.groups.into_iter().collect::<ParallelGroup>())?
                .optimize(false),
            &build.recent_blockhash,
            build.compute_unit_price_micro_lamports,
            build.compute_unit_min_priority_lamports,
        )
    }
}

#[wasm_bindgen]
pub fn create_deposits(
    deposits: Vec<CreateDepositParamsJs>,
    options: CreateDepositOptions,
) -> crate::Result<TransactionGroup> {
    create_deposits_builder(deposits, options)?.build_with_options(None, None)
}
