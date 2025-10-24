use std::collections::{HashMap, HashSet};

use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{
        token::PrepareTokenAccounts,
        user::PrepareUser,
        withdrawal::{CreateWithdrawal, CreateWithdrawalHint},
        StoreProgram,
    },
    js::instructions::BuildTransactionOptions,
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateWithdrawalParamsJs {
    pub market_token: StringPubkey,
    #[serde(default)]
    pub receiver: Option<StringPubkey>,
    #[serde(default)]
    pub long_receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub short_receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub long_swap_path: Vec<StringPubkey>,
    #[serde(default)]
    pub short_swap_path: Vec<StringPubkey>,
    #[serde(default)]
    pub market_token_amount: u64,
    #[serde(default)]
    pub min_long_receive_amount: u64,
    #[serde(default)]
    pub min_short_receive_amount: u64,
    #[serde(default)]
    pub unwrap_native_on_receive: bool,
}

#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateWithdrawalOptions {
    pub recent_blockhash: String,
    pub payer: StringPubkey,
    #[serde(default)]
    pub program: Option<StoreProgram>,
    #[serde(default)]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    pub compute_unit_min_priority_lamports: Option<u64>,
    pub hints: HashMap<StringPubkey, CreateWithdrawalHint>,
    #[serde(default)]
    pub transaction_group: TransactionGroupOptions,
}

#[wasm_bindgen]
pub struct CreateWithdrawalsBuilder {
    payer: StringPubkey,
    tokens: HashSet<StringPubkey>,
    groups: Vec<AtomicGroup>,
    transaction_group: TransactionGroupOptions,
    build: BuildTransactionOptions,
}

#[wasm_bindgen]
pub fn create_withdrawals_builder(
    withdrawals: Vec<CreateWithdrawalParamsJs>,
    options: CreateWithdrawalOptions,
) -> crate::Result<CreateWithdrawalsBuilder> {
    let mut tokens = HashSet::default();
    let mut groups: Vec<AtomicGroup> = Vec::with_capacity(withdrawals.len());

    for params in withdrawals.into_iter() {
        let market_token = params.market_token;
        let hint = options.hints.get(&market_token).ok_or_else(|| {
            crate::Error::custom(format!("hint for {} is not provided", market_token.0))
        })?;

        tokens.insert(market_token);

        let program = options.program.clone().unwrap_or_default();
        let builder = CreateWithdrawal::builder()
            .program(program)
            .payer(options.payer)
            .market_token(market_token)
            .long_receive_token(params.long_receive_token)
            .short_receive_token(params.short_receive_token)
            .long_swap_path(params.long_swap_path)
            .short_swap_path(params.short_swap_path)
            .market_token_amount(params.market_token_amount)
            .min_long_receive_amount(params.min_long_receive_amount)
            .min_short_receive_amount(params.min_short_receive_amount)
            .unwrap_native_on_receive(params.unwrap_native_on_receive);

        let built = if let Some(r) = params.receiver {
            builder.receiver(r).build()
        } else {
            builder.build()
        };

        let ag = built.into_atomic_group(hint)?;
        groups.push(ag);
    }

    Ok(CreateWithdrawalsBuilder {
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
impl CreateWithdrawalsBuilder {
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
pub fn create_withdrawals(
    withdrawals: Vec<CreateWithdrawalParamsJs>,
    options: CreateWithdrawalOptions,
) -> crate::Result<TransactionGroup> {
    create_withdrawals_builder(withdrawals, options)?.build_with_options(None, None)
}
