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

/// Builder for one signed Cancel transaction group.
#[derive(Debug, Clone, TypedBuilder)]
pub struct Cancel {
    /// Deployment selected for the signed domain.
    #[builder(default)]
    pub program: EvmProxyProgram,
    /// Transaction fee payer and submitter.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Fields bound by the signature.
    pub intent: crate::evm_proxy::CancelIntent,
    /// Compact ECDSA signature.
    pub signature: [u8; 64],
    /// Canonical recovery ID, zero or one.
    pub recovery_id: u8,
}

impl gmsol_solana_utils::IntoAtomicGroup for Cancel {
    type Hint = ();

    fn into_atomic_group(
        self,
        _hint: &(),
    ) -> gmsol_solana_utils::Result<gmsol_solana_utils::AtomicGroup> {
        let instruction = crate::evm_proxy::cancel_instruction(
            self.program.id.0,
            self.payer.0,
            &self.intent,
            self.signature,
            self.recovery_id,
        )
        .map_err(gmsol_solana_utils::Error::custom)?;
        Ok(gmsol_solana_utils::AtomicGroup::with_instructions(
            &self.payer.0,
            Some(instruction),
        ))
    }
}
