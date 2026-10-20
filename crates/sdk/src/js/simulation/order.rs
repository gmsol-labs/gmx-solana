//! Order simulation.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::{
    builders::order::{CreateOrderKind, CreateOrderParams},
    js::simulation::{encode_borsh_base64, encode_bytemuck_base64},
    serde::StringPubkey,
    simulation::{
        order::OrderSimulationOutput, BuilderFeeOutcome as SimBuilderFee,
        TokenReceipt as SimTokenReceipt,
    },
};

use crate::js::position::JsPositionModel;

/// Arguments for order simulation.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulateOrderArgs {
    pub(crate) kind: CreateOrderKind,
    pub(crate) params: CreateOrderParams,
    pub(crate) collateral_or_swap_out_token: StringPubkey,
    #[serde(default)]
    pub(crate) pay_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) receive_token: Option<StringPubkey>,
    #[serde(default)]
    pub(crate) swap_path: Option<Vec<StringPubkey>>,
    #[serde(default)]
    pub(crate) prefer_swap_out_token_update: Option<bool>,
    #[serde(default)]
    pub(crate) skip_limit_price_validation: Option<bool>,
    #[serde(default)]
    pub(crate) limit_swap_slippage: Option<u128>,
    #[serde(default)]
    pub(crate) update_prices_for_limit_order: Option<bool>,
    /// The builder fee factor checkpointed onto the order. Absent or `0` means
    /// the order has no builder fee.
    ///
    /// Read it from the order account (`Order.builder_fee_factor`) for an order
    /// that is already checkpointed: that is what the program charges, not the
    /// factor the builder advertises now. For an order not yet checkpointed, use
    /// the factor the builder advertises on its User Account, which is what
    /// `set_builder_fee` requires as `expected_factor`.
    ///
    /// Only `Simulator.simulate_order` models it. The deprecated
    /// `MarketGraph.simulate_order` rejects a non-zero value. Position orders
    /// only: a swap order with a non-zero factor is rejected.
    #[serde(default)]
    pub(crate) builder_fee_factor: Option<u128>,
}

/// A builder fee computed for a simulated execution.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SimulatedBuilderFee {
    /// The checkpointed factor that was applied.
    factor: u128,
    /// The token the fee is denominated in.
    token: StringPubkey,
    /// The computed amount, before clamping to the available output.
    payable_amount: u128,
    /// The amount actually charged.
    paid_amount: u128,
}

impl From<&SimBuilderFee> for SimulatedBuilderFee {
    fn from(fee: &SimBuilderFee) -> Self {
        Self {
            factor: fee.factor,
            token: fee.token.into(),
            payable_amount: fee.payable_amount,
            paid_amount: fee.paid_amount.into(),
        }
    }
}

/// What the owner ends up holding in one token after a decrease.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SimulatedTokenReceipt {
    /// The token.
    token: StringPubkey,
    /// The amount paid out in this token by the execution (trade outputs and
    /// funding rebates), before the builder fee.
    gross_amount: u128,
    /// The builder fee reserved from this token.
    fee_amount: u128,
    /// `gross_amount - fee_amount`.
    net_amount: u128,
}

impl From<&SimTokenReceipt> for SimulatedTokenReceipt {
    fn from(receipt: &SimTokenReceipt) -> Self {
        Self {
            token: receipt.token.into(),
            gross_amount: receipt.gross_amount,
            fee_amount: receipt.fee_amount.into(),
            net_amount: receipt.net_amount,
        }
    }
}

/// A JS binding for [`OrderSimulationOutput`].
#[wasm_bindgen(js_name = OrderSimulationOutput)]
pub struct JsOrderSimulationOutput {
    pub(crate) output: OrderSimulationOutput,
}

/// Simulation output for increase order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct IncreaseOrderSimulationOutput {
    swaps: Vec<String>,
    /// The model's report. Its `collateral_increment_amount` is the net
    /// collateral the position received.
    report: String,
    position: Option<String>,
    /// The collateral produced by the pay-token swap, before the builder fee.
    gross_collateral_amount: u128,
    /// The collateral the position received: `gross_collateral_amount` minus
    /// the builder fee.
    net_collateral_amount: u128,
    /// The builder fee. Absent when the order has none.
    builder_fee: Option<SimulatedBuilderFee>,
}

/// Simulation output for decrease order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DecreaseOrderSimulationOutput {
    swaps: Vec<String>,
    /// The model's report. It holds the gross payout: the builder fee is not
    /// netted from it. See `receipts`.
    report: String,
    position: Option<String>,
    decrease_swap: Option<String>,
    /// What the owner holds per token, with the builder fee taken out of the
    /// final output token. Counts everything the execution pays out: the final
    /// output, the secondary (pnl token) output and the funding rebates. Amounts
    /// in the same token share one entry. The final output token comes first.
    receipts: Vec<SimulatedTokenReceipt>,
    /// The builder fee. Absent when the order has none.
    builder_fee: Option<SimulatedBuilderFee>,
}

/// Simulation output for swap order.
#[derive(Debug, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SwapOrderSimulationOutput {
    output_token: StringPubkey,
    amount: u128,
    report: Vec<String>,
}

#[wasm_bindgen(js_class = OrderSimulationOutput)]
impl JsOrderSimulationOutput {
    /// Returns increase order simulation output.
    pub fn increase(
        &self,
        skip_position: Option<bool>,
    ) -> crate::Result<Option<IncreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Increase {
            swaps,
            report,
            position,
            gross_collateral_amount,
            builder_fee,
        } = &self.output
        {
            let encode_position = !skip_position.unwrap_or_default();
            Ok(Some(IncreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_position.then(|| encode_bytemuck_base64(position.position())),
                gross_collateral_amount: *gross_collateral_amount,
                net_collateral_amount: *report.params().collateral_increment_amount(),
                builder_fee: builder_fee.as_ref().map(Into::into),
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns decrease order simulation output.
    pub fn decrease(
        &self,
        skip_position: Option<bool>,
    ) -> crate::Result<Option<DecreaseOrderSimulationOutput>> {
        if let OrderSimulationOutput::Decrease {
            swaps,
            report,
            position,
            receipts,
            builder_fee,
        } = &self.output
        {
            let encode_position = !skip_position.unwrap_or_default();
            Ok(Some(DecreaseOrderSimulationOutput {
                swaps: swaps
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
                report: encode_borsh_base64(report)?,
                position: encode_position.then(|| encode_bytemuck_base64(position.position())),
                decrease_swap: position
                    .swap_history()
                    .first()
                    .map(|s| encode_borsh_base64(&**s))
                    .transpose()?,
                receipts: receipts.iter().map(Into::into).collect(),
                builder_fee: builder_fee.as_ref().map(Into::into),
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns swap order simulation output.
    pub fn swap(&self) -> crate::Result<Option<SwapOrderSimulationOutput>> {
        if let OrderSimulationOutput::Swap(swap) = &self.output {
            Ok(Some(SwapOrderSimulationOutput {
                output_token: (*swap.output_token()).into(),
                amount: swap.amount(),
                report: swap
                    .reports()
                    .iter()
                    .map(encode_borsh_base64)
                    .collect::<crate::Result<Vec<_>>>()?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns the result position model.
    pub fn position_model(&self) -> Option<JsPositionModel> {
        match &self.output {
            OrderSimulationOutput::Increase { position, .. }
            | OrderSimulationOutput::Decrease { position, .. } => {
                Some(JsPositionModel::from(position.clone()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &str) -> SimulateOrderArgs {
        serde_json::from_str(&format!(
            r#"{{
                "kind": "MarketIncrease",
                "params": {{
                    "market_token": "11111111111111111111111111111111",
                    "is_long": true,
                    "size": 1000,
                    "amount": 10
                }},
                "collateral_or_swap_out_token": "11111111111111111111111111111111"
                {extra}
            }}"#
        ))
        .unwrap()
    }

    #[test]
    fn the_builder_fee_keeps_each_amount_in_its_own_field() {
        let token = solana_sdk::pubkey::Pubkey::new_unique();
        let fee = SimulatedBuilderFee::from(&SimBuilderFee {
            factor: 7,
            token,
            payable_amount: 100,
            paid_amount: 60,
        });
        assert_eq!(fee.factor, 7);
        assert_eq!(*fee.token, token);
        assert_eq!(fee.payable_amount, 100, "computed, before clamping");
        assert_eq!(fee.paid_amount, 60, "charged");
    }

    #[test]
    fn the_receipt_keeps_gross_fee_and_net_apart() {
        let token = solana_sdk::pubkey::Pubkey::new_unique();
        let receipt = SimulatedTokenReceipt::from(&SimTokenReceipt {
            token,
            gross_amount: 100,
            fee_amount: u64::MAX,
            net_amount: 40,
        });
        assert_eq!(*receipt.token, token);
        assert_eq!(receipt.gross_amount, 100);
        assert_eq!(
            receipt.fee_amount,
            u128::from(u64::MAX),
            "widened, not truncated"
        );
        assert_eq!(receipt.net_amount, 40);
    }

    #[test]
    fn builder_fee_factor_is_optional() {
        assert_eq!(args("").builder_fee_factor, None);
    }

    #[test]
    fn builder_fee_factor_is_read() {
        let factor = 1_000_000_000_000_000_000u128;
        assert_eq!(
            args(&format!(r#", "builder_fee_factor": {factor}"#)).builder_fee_factor,
            Some(factor)
        );
        assert_eq!(
            args(r#", "builder_fee_factor": 0"#).builder_fee_factor,
            Some(0)
        );
    }
}
