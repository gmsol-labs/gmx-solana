use gmsol_solana_utils::Program;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::serde::StringPubkey;

/// EVM proxy deployment used by instruction builders.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct EvmProxyProgram {
    /// Selected deployment's program ID.
    #[builder(setter(into))]
    pub id: StringPubkey,
}

impl EvmProxyProgram {
    /// Find the canonical CPI event authority for this deployment.
    pub fn find_event_authority_address(&self) -> Pubkey {
        crate::pda::find_event_authority_address(&self.id).0
    }
}

impl Default for EvmProxyProgram {
    fn default() -> Self {
        gmsol_programs::gmsol_evm_proxy::ID.into()
    }
}

impl From<Pubkey> for EvmProxyProgram {
    fn from(id: Pubkey) -> Self {
        Self { id: id.into() }
    }
}

impl gmsol_programs::anchor_lang::Id for EvmProxyProgram {
    fn id() -> Pubkey {
        gmsol_programs::gmsol_evm_proxy::ID
    }
}

impl Program for EvmProxyProgram {
    fn id(&self) -> &Pubkey {
        &self.id
    }
}
