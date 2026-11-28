use gmsol_evm_proxy_intent::ChainId;
use gmsol_programs::gmsol_evm_proxy::client::{accounts, args};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, Program, ProgramExt};
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

/// Builder for a signed Cancel.
#[derive(Debug, Clone, TypedBuilder)]
pub struct Cancel {
    /// Deployment selected for the signed domain.
    #[builder(default)]
    pub program: EvmProxyProgram,
    /// Transaction fee payer and submitter.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// EVM chain the intent is signed for.
    pub chain_id: ChainId,
    /// Signed Cancel fields.
    pub intent: gmsol_evm_proxy_intent::Cancel,
    /// Compact ECDSA `r || s` signature.
    pub signature: [u8; 64],
    /// Recovery ID of the signature.
    pub recovery_id: u8,
}

impl IntoAtomicGroup for Cancel {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let payer = self.payer.0;
        let program_id = *self.program.id;
        let instruction = self
            .program
            .anchor_instruction(args::Cancel {
                chain_id: self.chain_id,
                nonce: self.intent.nonce,
                deadline: self.intent.deadline,
                signature: self.signature,
                recovery_id: self.recovery_id,
            })
            .anchor_accounts(
                accounts::Cancel {
                    submitter: payer,
                    config: crate::pda::find_proxy_config_address(&program_id).0,
                    proxy: self.intent.proxy_account,
                    event_authority: self.program.find_event_authority_address(),
                    program: program_id,
                },
                false,
            )
            .build();
        Ok(AtomicGroup::with_instructions(&payer, Some(instruction)))
    }
}
