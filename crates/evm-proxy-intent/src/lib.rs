//! Canonical EIP-712 encoding for EVM proxy intents.

use anchor_lang::{prelude::Pubkey, solana_program::keccak};

/// Domain name shared by every proxy intent.
pub const DOMAIN_NAME: &str = "GMX Solana EVM Proxy";
/// Domain version shared by every proxy intent.
pub const DOMAIN_VERSION: &str = "1";
/// Exact primary type used by Cancel.
pub const CANCEL_TYPE: &str =
    "Cancel(bytes32 proxyAccount,uint64 generation,uint64 nonce,uint64 deadline)";
const DOMAIN_TYPE: &[u8] = b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)";

fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

/// Hash the Cancel fields without changing the existing signed bytes.
pub fn cancel_struct_hash(proxy: &Pubkey, generation: u64, nonce: u64, deadline: u64) -> [u8; 32] {
    let generation = u64_word(generation);
    let nonce = u64_word(nonce);
    let deadline = u64_word(deadline);
    keccak::hashv(&[
        keccak::hash(CANCEL_TYPE.as_bytes()).as_ref(),
        proxy.as_ref(),
        &generation,
        &nonce,
        &deadline,
    ])
    .to_bytes()
}

/// Domain salt ties signatures to one deployment, Store and Solana cluster.
pub fn domain_salt(program: &Pubkey, store: &Pubkey, genesis_hash: &[u8; 32]) -> [u8; 32] {
    keccak::hashv(&[program.as_ref(), store.as_ref(), genesis_hash]).to_bytes()
}

/// EIP-712 domain separator for the exact configured chain and deployment.
pub fn domain_separator(
    program: &Pubkey,
    store: &Pubkey,
    genesis_hash: &[u8; 32],
    chain_id: &[u8; 32],
) -> [u8; 32] {
    let salt = domain_salt(program, store, genesis_hash);
    keccak::hashv(&[
        keccak::hash(DOMAIN_TYPE).as_ref(),
        keccak::hash(DOMAIN_NAME.as_bytes()).as_ref(),
        keccak::hash(DOMAIN_VERSION.as_bytes()).as_ref(),
        chain_id,
        &salt,
    ])
    .to_bytes()
}

/// Digest recovered by the on-chain verifier and exposed to SDK tests.
pub fn intent_digest(domain: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
    keccak::hashv(&[b"\x19\x01", domain, struct_hash]).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn field<const N: usize>(value: &Value, key: &str) -> [u8; N] {
        let encoded = value[key].as_str().unwrap().strip_prefix("0x").unwrap();
        let bytes = hex::decode(encoded).unwrap();
        bytes.try_into().unwrap()
    }

    #[test]
    fn independent_viem_vectors_match_cancel_encoding() {
        let fixture: Value =
            serde_json::from_str(include_str!("../fixtures/cancel-pair-vectors.json")).unwrap();
        assert_eq!(fixture["generator"], "viem@2.57.7");
        assert_eq!(fixture["vectors"].as_array().unwrap().len(), 22);
        for vector in fixture["vectors"].as_array().unwrap() {
            let program = Pubkey::new_from_array(field(vector, "program"));
            let store = Pubkey::new_from_array(field(vector, "store"));
            let genesis = field(vector, "genesis");
            let chain_id = field(vector, "chainId");
            let proxy = Pubkey::new_from_array(field(vector, "proxyAccount"));
            let generation = vector["generation"].as_str().unwrap().parse().unwrap();
            let nonce = vector["nonce"].as_str().unwrap().parse().unwrap();
            let deadline = vector["deadline"].as_str().unwrap().parse().unwrap();
            let domain = domain_separator(&program, &store, &genesis, &chain_id);
            let structure = cancel_struct_hash(&proxy, generation, nonce, deadline);
            if matches!(
                vector["name"].as_str(),
                Some("other-name" | "other-version")
            ) {
                assert_ne!(
                    domain,
                    field(vector, "domainSeparator"),
                    "{}",
                    vector["name"]
                );
                assert_ne!(
                    intent_digest(&domain, &structure),
                    field(vector, "digest"),
                    "{}",
                    vector["name"]
                );
                continue;
            }
            assert_eq!(
                domain,
                field(vector, "domainSeparator"),
                "{}",
                vector["name"]
            );
            assert_eq!(structure, field(vector, "structHash"), "{}", vector["name"]);
            assert_eq!(
                intent_digest(&domain, &structure),
                field(vector, "digest"),
                "{}",
                vector["name"]
            );
        }
    }
}
