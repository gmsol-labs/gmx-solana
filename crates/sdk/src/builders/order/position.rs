use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::CreateOrderParams as StoreCreateOrderParams,
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use solana_sdk::{pubkey::Pubkey, system_program};
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

use super::{CreateOrderHint, CreateOrderKind, CreateOrderParams};

/// Compute budget for LP token staking operations
const PREPARE_POSITION_COMPUTE_BUDGET: u32 = 15_000;

/// Builder for the `prepare_position` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct PreparePosition {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Order Kind.
    pub kind: CreateOrderKind,
    /// Collateral token.
    #[builder(setter(into))]
    pub collateral_token: StringPubkey,
    /// Order Parameters.
    pub params: CreateOrderParams,
    /// Execution lamports.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub execution_lamports: u64,
    /// Swap path length.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub swap_path_length: u8,
    /// Whether to unwrap the native token.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub should_unwrap_native_token: bool,
}

impl PreparePosition {
    /// Get position address.
    pub fn position_address(&self) -> Pubkey {
        self.program.find_position_address(
            &self.payer,
            &self.params.market_token,
            &self.collateral_token,
            self.params.is_long,
        )
    }
}

impl IntoAtomicGroup for PreparePosition {
    type Hint = CreateOrderHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let owner = &self.payer;
        let market_token = &self.params.market_token;
        let market = self.program.find_market_address(market_token);
        let collateral_token = &self.collateral_token;
        let position = self.position_address();
        let params = StoreCreateOrderParams {
            kind: self.kind.into(),
            decrease_position_swap_type: self.params.decrease_position_swap_type.map(Into::into),
            execution_lamports: self.execution_lamports,
            swap_path_length: self.swap_path_length,
            initial_collateral_delta_amount: self
                .params
                .amount
                .try_into()
                .map_err(crate::SolanaUtilsError::custom)?,
            size_delta_value: self.params.size,
            is_long: self.params.is_long,
            is_collateral_long: hint.is_collateral_long(collateral_token)?,
            min_output: Some(self.params.min_output),
            trigger_price: self.params.trigger_price,
            acceptable_price: self.params.acceptable_price,
            should_unwrap_native_token: self.should_unwrap_native_token,
            valid_from_ts: self.params.valid_from_ts,
        };

        let prepare = self
            .program
            .anchor_instruction(args::PreparePosition { params })
            .anchor_accounts(
                accounts::PreparePosition {
                    owner: **owner,
                    store: self.program.store.0,
                    market,
                    position,
                    system_program: system_program::ID,
                },
                true,
            )
            .build();
        let mut ag = AtomicGroup::with_instructions(&self.payer, [prepare]);
        ag.compute_budget_mut()
            .with_limit(PREPARE_POSITION_COMPUTE_BUDGET);
        Ok(ag)
    }
}
