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
        let fields = gmsol_evm_proxy_intent::cancel_struct_hash(&proxy, self.nonce, self.deadline);
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
            nonce: intent.nonce,
            deadline: intent.deadline,
            signature,
            recovery_id,
        }
        .data(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmsol_programs::anchor_lang::AnchorDeserialize;
    use serde_json::Value;

    fn bytes<const N: usize>(value: &str) -> [u8; N] {
        hex::decode(value.strip_prefix("0x").unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    #[test]
    fn cancel_vectors_bind_nonce_typed_data_and_instruction() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../evm-proxy-intent/fixtures/cancel-pair-vectors.json"
        ))
        .unwrap();
        for name in ["first", "max", "zero", "large-nonce"] {
            let vector = fixture["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .unwrap();
            let program = Pubkey::new_from_array(bytes(vector["program"].as_str().unwrap()));
            let store = Pubkey::new_from_array(bytes(vector["store"].as_str().unwrap()));
            let genesis = bytes(vector["genesis"].as_str().unwrap());
            let intent = CancelIntent {
                evm_address: bytes(fixture["evmAddress"].as_str().unwrap()),
                chain_id: bytes(vector["chainId"].as_str().unwrap()),
                nonce: vector["nonce"].as_str().unwrap().parse().unwrap(),
                deadline: vector["deadline"].as_str().unwrap().parse().unwrap(),
            };
            let typed = intent.typed_data(&program, &store, &genesis);
            assert_eq!(
                typed.type_signature,
                "Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)"
            );
            assert_eq!(typed.nonce, intent.nonce);
            assert_eq!(
                typed.proxy_account.to_bytes(),
                bytes::<32>(vector["proxyAccount"].as_str().unwrap())
            );
            assert_eq!(
                intent.digest(&program, &store, &genesis),
                bytes::<32>(vector["digest"].as_str().unwrap())
            );
            let submitter = Pubkey::new_unique();
            let ix = cancel_instruction(program, submitter, &intent, [1; 64], 0).unwrap();
            assert_eq!(ix.data.len(), 141);
            let decoded = args::Cancel::deserialize(&mut &ix.data[8..]).unwrap();
            assert_eq!(
                (decoded.nonce, decoded.deadline),
                (intent.nonce, intent.deadline)
            );
            assert_eq!(ix.accounts[0].pubkey, submitter);
            assert!(ix.accounts[0].is_signer);
            assert_eq!(
                ix.accounts[1].pubkey,
                crate::pda::find_proxy_config_address(&program).0
            );
            assert_eq!(ix.accounts[2].pubkey, typed.proxy_account);
        }
    }
}
