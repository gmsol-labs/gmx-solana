use std::collections::HashSet;

use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ParallelGroup};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::{shift::CreateShift, token::PrepareTokenAccounts, user::PrepareUser, StoreProgram},
    js::instructions::BuildTransactionOptions,
    serde::StringPubkey,
};

use super::{TransactionGroup, TransactionGroupOptions};

#[derive(Debug, Serialize, Deserialize, Tsify, Clone)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateShiftParamsJs {
    pub from_market_token: StringPubkey,
    pub to_market_token: StringPubkey,
    #[serde(default)]
    pub receiver: Option<StringPubkey>,
    #[serde(default)]
    pub from_market_token_amount: Option<u128>,
    #[serde(default)]
    pub min_to_market_token_amount: Option<u128>,
    #[serde(default)]
    pub skip_to_market_token_ata_creation: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CreateShiftOptions {
    pub recent_blockhash: String,
    pub payer: StringPubkey,
    #[serde(default)]
    pub program: Option<StoreProgram>,
    #[serde(default)]
    pub compute_unit_price_micro_lamports: Option<u64>,
    #[serde(default)]
    pub compute_unit_min_priority_lamports: Option<u64>,
    #[serde(default)]
    pub transaction_group: TransactionGroupOptions,
}

#[wasm_bindgen]
pub struct CreateShiftsBuilder {
    payer: StringPubkey,
    tokens: HashSet<StringPubkey>,
    groups: Vec<AtomicGroup>,
    transaction_group: TransactionGroupOptions,
    build: BuildTransactionOptions,
}

#[wasm_bindgen]
pub fn create_shifts_builder(
    shifts: Vec<CreateShiftParamsJs>,
    options: CreateShiftOptions,
) -> crate::Result<CreateShiftsBuilder> {
    let mut tokens = HashSet::default();
    let mut groups: Vec<AtomicGroup> = Vec::with_capacity(shifts.len());

    for params in shifts.into_iter() {
        tokens.insert(params.from_market_token);
        tokens.insert(params.to_market_token);

        let program = options.program.clone().unwrap_or_default();
        let builder = CreateShift::builder()
            .program(program)
            .payer(options.payer)
            .from_market_token(params.from_market_token)
            .to_market_token(params.to_market_token)
            .from_market_token_amount(
                params
                    .from_market_token_amount
                    .unwrap_or_default()
                    .try_into()?,
            )
            .min_to_market_token_amount(
                params
                    .min_to_market_token_amount
                    .unwrap_or_default()
                    .try_into()?,
            )
            .skip_to_market_token_ata_creation(
                params.skip_to_market_token_ata_creation.unwrap_or_default(),
            );

        let built = if let Some(r) = params.receiver {
            builder.receiver(r).build()
        } else {
            builder.build()
        };

        let ag = built.into_atomic_group(&())?;
        groups.push(ag);
    }

    Ok(CreateShiftsBuilder {
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
impl CreateShiftsBuilder {
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
pub fn create_shifts(
    shifts: Vec<CreateShiftParamsJs>,
    options: CreateShiftOptions,
) -> crate::Result<TransactionGroup> {
    create_shifts_builder(shifts, options)?.build_with_options(None, None)
}
