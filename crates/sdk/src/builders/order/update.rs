use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types,
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup};
use typed_builder::TypedBuilder;

use crate::{
    builders::{
        callback::{Callback, CallbackParams},
        StoreProgram,
    },
    serde::StringPubkey,
};

/// Builder for the `update_order` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateOrder {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Order.
    #[builder(setter(into))]
    pub order: StringPubkey,
    /// Parameters.
    pub params: UpdateOrderParams,
}

/// Parameters for creating an order.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateOrderParams {
    /// Size delta value.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option))]
    pub size_delta_value: Option<u128>,
    /// Acceptable price.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option))]
    pub acceptable_price: Option<u128>,
    /// Trigger price.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option))]
    pub trigger_price: Option<u128>,
    /// Min output.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option))]
    pub min_output: Option<u128>,
    /// Valid from this timestamp.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option))]
    pub valid_from_ts: Option<i64>,
}

impl From<UpdateOrderParams> for types::UpdateOrderParams {
    fn from(params: UpdateOrderParams) -> Self {
        Self {
            size_delta_value: params.size_delta_value,
            acceptable_price: params.acceptable_price,
            trigger_price: params.trigger_price,
            min_output: params.min_output,
            valid_from_ts: params.valid_from_ts,
        }
    }
}

/// Hint for [`UpdateOrder`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct UpdateOrderHint {
    /// Market token.
    #[builder(setter(into))]
    pub market_token: StringPubkey,
    /// Callback.
    pub callback: Option<Callback>,
}

impl IntoAtomicGroup for UpdateOrder {
    type Hint = UpdateOrderHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;

        let CallbackParams {
            callback_authority,
            callback_program,
            callback_shared_data_account,
            callback_partitioned_data_account,
            ..
        } = self.program.get_callback_params(hint.callback.as_ref());

        let update = self
            .program
            .instruction(args::UpdateOrderV2 {
                params: self.params.into(),
            })
            .accounts(
                accounts::UpdateOrderV2 {
                    owner: payer,
                    store: self.program.store.0,
                    market: self.program.find_market_address(&hint.market_token),
                    order: self.order.0,
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                    callback_authority,
                    callback_program,
                    callback_shared_data_account,
                    callback_partitioned_data_account,
                },
                true,
            )
            .build();
        Ok(AtomicGroup::with_instructions(&payer, Some(update)))
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use crate::constants;

    use super::*;

    #[test]
    fn update_order() -> crate::Result<()> {
        let payer = Pubkey::new_unique();
        let order = Pubkey::new_unique();
        let market_token = Pubkey::new_unique();
        let params = UpdateOrderParams::builder()
            .size_delta_value(1_000 * constants::MARKET_USD_UNIT)
            .build();
        UpdateOrder::builder()
            .payer(payer)
            .order(order)
            .params(params)
            .build()
            .into_atomic_group(
                &UpdateOrderHint::builder()
                    .market_token(market_token)
                    .callback(None)
                    .build(),
            )?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
            )?;
        Ok(())
    }
}
