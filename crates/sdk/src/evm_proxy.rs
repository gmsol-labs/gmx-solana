//! Signed EVM proxy intent data and instructions.

use gmsol_programs::{
    anchor_lang::{InstructionData, ToAccountMetas},
    gmsol_evm_proxy::client::{accounts, args},
};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Signed fields for one Cancel operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelIntent {
    pub evm_address: [u8; 20],
    pub chain_id: [u8; 32],
    pub generation: u64,
    pub nonce: u64,
    pub deadline: u64,
}

/// Exact EIP-712 fields presented to the EVM signer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelTypedData {
    pub name: &'static str,
    pub version: &'static str,
    pub chain_id: [u8; 32],
    pub salt: [u8; 32],
    pub type_signature: &'static str,
    pub proxy_account: Pubkey,
    pub generation: u64,
    pub nonce: u64,
    pub deadline: u64,
}

impl CancelIntent {
    /// Build typed fields from the same canonical encoding used by the program.
    pub fn typed_data(
        &self,
        program_id: &Pubkey,
        store: &Pubkey,
        genesis_hash: &[u8; 32],
    ) -> CancelTypedData {
        CancelTypedData {
            name: gmsol_evm_proxy_intent::DOMAIN_NAME,
            version: gmsol_evm_proxy_intent::DOMAIN_VERSION,
            chain_id: self.chain_id,
            salt: gmsol_evm_proxy_intent::domain_salt(program_id, store, genesis_hash),
            type_signature: gmsol_evm_proxy_intent::CANCEL_TYPE,
            proxy_account: crate::pda::find_proxy_address(&self.evm_address, program_id).0,
            generation: self.generation,
            nonce: self.nonce,
            deadline: self.deadline,
        }
    }

    /// Digest that the on-chain verifier recovers against the EVM address.
    pub fn digest(&self, program_id: &Pubkey, store: &Pubkey, genesis_hash: &[u8; 32]) -> [u8; 32] {
        let proxy = crate::pda::find_proxy_address(&self.evm_address, program_id).0;
        let domain = gmsol_evm_proxy_intent::domain_separator(
            program_id,
            store,
            genesis_hash,
            &self.chain_id,
        );
        let fields = gmsol_evm_proxy_intent::cancel_struct_hash(
            &proxy,
            self.generation,
            self.nonce,
            self.deadline,
        );
        gmsol_evm_proxy_intent::intent_digest(&domain, &fields)
    }
}

/// Build a Cancel instruction against the canonical Config and proxy PDAs.
pub fn cancel_instruction(
    program_id: Pubkey,
    submitter: Pubkey,
    intent: &CancelIntent,
    signature: [u8; 64],
    recovery_id: u8,
) -> crate::Result<Instruction> {
    if recovery_id > 1 {
        return Err(crate::Error::custom("invalid EVM recovery ID"));
    }
    Ok(Instruction {
        program_id,
        accounts: accounts::Cancel {
            submitter,
            config: crate::pda::find_proxy_config_address(&program_id).0,
            proxy: crate::pda::find_proxy_address(&intent.evm_address, &program_id).0,
        }
        .to_account_metas(None),
        data: args::Cancel {
            evm_address: intent.evm_address,
            chain_id: intent.chain_id,
            generation: intent.generation,
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        }
        .data(),
    })
}
