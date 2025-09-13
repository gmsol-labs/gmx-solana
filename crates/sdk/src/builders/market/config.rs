use gmsol_programs::gmsol_store::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use gmsol_utils::market::{MarketConfigFactor, MarketConfigFlag};
use indexmap::IndexMap;
use typed_builder::TypedBuilder;

use crate::{builders::StoreProgram, serde::StringPubkey};

/// Builder for `udpate_closed_state` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct SetMarketConfigUpdatable {
    /// Payer (a.k.a. authority).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Store program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub store_program: StoreProgram,
    /// Flags.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub flags: IndexMap<MarketConfigFlag, bool>,
    /// Factors.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub factors: IndexMap<MarketConfigFactor, bool>,
}

impl IntoAtomicGroup for SetMarketConfigUpdatable {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let flags = self
            .flags
            .iter()
            .map(|(flag, updatable)| args::SetMarketConfigUpdatable {
                is_flag: true,
                key: flag.to_string(),
                updatable: *updatable,
            });
        let factors =
            self.factors
                .iter()
                .map(|(factor, updatable)| args::SetMarketConfigUpdatable {
                    is_flag: false,
                    key: factor.to_string(),
                    updatable: *updatable,
                });
        let ixs = flags.chain(factors).map(|args| {
            self.store_program
                .anchor_instruction(args)
                .anchor_accounts(
                    accounts::SetMarketConfigUpdatable {
                        authority: self.payer.0,
                        store: self.store_program.store.0,
                    },
                    false,
                )
                .build()
        });
        Ok(AtomicGroup::with_instructions(&self.payer, ixs))
    }
}
