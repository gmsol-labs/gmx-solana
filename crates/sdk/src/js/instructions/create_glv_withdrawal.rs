use std::collections::{HashMap, HashSet};

use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{
        glv_withdrawal::{CreateGlvWithdrawal, CreateGlvWithdrawalHint},
        token::PrepareTokenAccounts,
        user::PrepareUser,
        StoreProgram,
    },
    js::instructions::BuildTransactionOptions,
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateGlvWithdrawalParamsJs {
    pub glv_token: StringPubkey,
    pub market_token: StringPubkey,
    #[serde(default)]
    pub receiver: Option<StringPubkey>,
    #[serde(default)]
    pub long_receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub short_receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub long_swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub short_swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub glv_token_amount: Option<u128>,
    #[serde(default)]
    pub min_long_receive_amount: Option<u128>,
    #[serde(default)]
    pub min_short_receive_amount: Option<u128>,
    #[serde(default)]
    pub skip_unwrap_native_on_receive: Option<bool>,
    #[serde(default)]
    pub skip_long_receive_token_ata_creation: Option<bool>,
    #[serde(default)]
    pub skip_short_receive_token_ata_creation: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateGlvWithdrawalOptions {
    pub recent_blockhash: String,
    pub payer: StringPubkey,
    #[serde(default)]
    pub program: Option<StoreProgram>,
    #[serde(default)]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    pub compute_unit_min_priority_lamports: Option<u64>,
    pub hints: HashMap<StringPubkey, CreateGlvWithdrawalHint>,
    #[serde(default)]
    pub transaction_group: TransactionGroupOptions,
}

#[wasm_bindgen]
pub struct CreateGlvWithdrawalsBuilder {
    payer: StringPubkey,
    tokens: HashSet<StringPubkey>,
    groups: Vec<AtomicGroup>,
    transaction_group: TransactionGroupOptions,
    build: BuildTransactionOptions,
}

#[wasm_bindgen]
pub fn create_glv_withdrawals_builder(
    withdrawals: Vec<CreateGlvWithdrawalParamsJs>,
    options: CreateGlvWithdrawalOptions,
) -> crate::Result<CreateGlvWithdrawalsBuilder> {
    let mut tokens = HashSet::default();
    let mut groups: Vec<AtomicGroup> = Vec::with_capacity(withdrawals.len());

    for params in withdrawals.into_iter() {
        let glv_token = params.glv_token;
        let hint = options.hints.get(&glv_token).ok_or_else(|| {
            crate::Error::custom(format!("hint for {} is not provided", glv_token.0))
        })?;

        tokens.insert(params.market_token);
        if let Some(t) = params.long_receive_token.as_ref() {
            tokens.insert(*t);
        }
        if let Some(t) = params.short_receive_token.as_ref() {
            tokens.insert(*t);
        }

        let program = options.program.clone().unwrap_or_default();
        let builder = CreateGlvWithdrawal::builder()
            .program(program)
            .payer(options.payer)
            .glv_token(params.glv_token)
            .market_token(params.market_token)
            .long_receive_token(params.long_receive_token)
            .short_receive_token(params.short_receive_token)
            .long_swap_path(params.long_swap_path.unwrap_or_default())
            .short_swap_path(params.short_swap_path.unwrap_or_default())
            .glv_token_amount(params.glv_token_amount.unwrap_or_default().try_into()?)
            .min_long_receive_amount(
                params
                    .min_long_receive_amount
                    .unwrap_or_default()
                    .try_into()?,
            )
            .min_short_receive_amount(
                params
                    .min_short_receive_amount
                    .unwrap_or_default()
                    .try_into()?,
            )
            .unwrap_native_on_receive(!params.skip_unwrap_native_on_receive.unwrap_or_default())
            .skip_long_receive_token_ata_creation(
                params
                    .skip_long_receive_token_ata_creation
                    .unwrap_or_default(),
            )
            .skip_short_receive_token_ata_creation(
                params
                    .skip_short_receive_token_ata_creation
                    .unwrap_or_default(),
            );

        let built = if let Some(r) = params.receiver {
            builder.receiver(r).build()
        } else {
            builder.build()
        };

        let ag = built.into_atomic_group(hint)?;
        groups.push(ag);
    }

    Ok(CreateGlvWithdrawalsBuilder {
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
impl CreateGlvWithdrawalsBuilder {
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
pub fn create_glv_withdrawals(
    withdrawals: Vec<CreateGlvWithdrawalParamsJs>,
    options: CreateGlvWithdrawalOptions,
) -> crate::Result<TransactionGroup> {
    create_glv_withdrawals_builder(withdrawals, options)?.build_with_options(None, None)
}
