//! Canonical EIP-712 encoding for EVM proxy intents.

use anchor_lang::{prelude::Pubkey, solana_program::keccak};

/// Domain name shared by every proxy intent.
pub const DOMAIN_NAME: &str = "GMX Solana EVM Proxy";
/// Domain version shared by every proxy intent.
pub const DOMAIN_VERSION: &str = "1";
/// Exact primary type used by Cancel.
pub const CANCEL_TYPE: &str = "Cancel(bytes32 proxyAccount,uint64 nonce,uint64 deadline)";
/// Exact primary type used by Withdraw.
pub const WITHDRAW_TYPE: &str = "Withdraw(bytes32 proxyAccount,string mint,uint64 amount,uint8 decimals,string recipient,uint64 nonce,uint64 deadline)";
/// Exact primary type used by referral registration.
pub const SET_REFERRAL_CODE_TYPE: &str =
    "SetReferralCode(bytes32 proxyAccount,string code,uint64 nonce,uint64 deadline)";
const DOMAIN_TYPE: &[u8] = b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)";

fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn u8_word(value: u8) -> [u8; 32] {
    let mut word = [0; 32];
    word[31] = value;
    word
}

/// Store's registered eight-byte code, encoded without leading base58 ones.
/// The zero code encodes to an empty string and is rejected by registration.
pub fn canonical_referral_code(code: &[u8; 8]) -> String {
    bs58::encode(code)
        .into_string()
        .trim_start_matches('1')
        .to_owned()
}

/// Hash the canonical code rather than a caller-supplied string alias.
pub fn set_referral_code_struct_hash(
    proxy: &Pubkey,
    code: &[u8; 8],
    nonce: u64,
    deadline: u64,
) -> [u8; 32] {
    let code = canonical_referral_code(code);
    keccak::hashv(&[
        keccak::hash(SET_REFERRAL_CODE_TYPE.as_bytes()).as_ref(),
        proxy.as_ref(),
        keccak::hash(code.as_bytes()).as_ref(),
        &u64_word(nonce),
        &u64_word(deadline),
    ])
    .to_bytes()
}

/// Hash the canonical single-nonce Cancel fields.
pub fn cancel_struct_hash(proxy: &Pubkey, nonce: u64, deadline: u64) -> [u8; 32] {
    let nonce = u64_word(nonce);
    let deadline = u64_word(deadline);
    keccak::hashv(&[
        keccak::hash(CANCEL_TYPE.as_bytes()).as_ref(),
        proxy.as_ref(),
        &nonce,
        &deadline,
    ])
    .to_bytes()
}

/// Fields covered by the signed Withdraw schema.
pub struct WithdrawFields {
    pub proxy: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub decimals: u8,
    pub recipient: Pubkey,
    pub nonce: u64,
    pub deadline: u64,
}

/// Hash canonical account keys and the complete signed Withdraw nonce.
pub fn withdraw_struct_hash(fields: &WithdrawFields) -> [u8; 32] {
    let mint = fields.mint.to_string();
    let recipient = fields.recipient.to_string();
    let amount = u64_word(fields.amount);
    let decimals = u8_word(fields.decimals);
    let nonce = u64_word(fields.nonce);
    let deadline = u64_word(fields.deadline);
    keccak::hashv(&[
        keccak::hash(WITHDRAW_TYPE.as_bytes()).as_ref(),
        fields.proxy.as_ref(),
        keccak::hash(mint.as_bytes()).as_ref(),
        &amount,
        &decimals,
        keccak::hash(recipient.as_bytes()).as_ref(),
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
    fn independent_viem_vectors_match_referral_encoding() {
        let fixture: Value =
            serde_json::from_str(include_str!("../fixtures/referral-vectors.json")).unwrap();
        assert_eq!(fixture["generator"], "viem@2.57.7");
        assert_eq!(fixture["type"], SET_REFERRAL_CODE_TYPE);
        assert_eq!(fixture["vectors"].as_array().unwrap().len(), 24);
        let program = Pubkey::new_from_array(field(&fixture, "program"));
        let store = Pubkey::new_from_array(field(&fixture, "store"));
        let proxy = Pubkey::new_from_array(field(&fixture, "proxy"));
        for vector in fixture["vectors"].as_array().unwrap() {
            let code: [u8; 8] = hex::decode(vector["bytesHex"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap();
            assert_eq!(canonical_referral_code(&code), vector["canonical"]);
            let structure = set_referral_code_struct_hash(
                &proxy,
                &code,
                vector["nonce"].as_str().unwrap().parse().unwrap(),
                vector["deadline"].as_str().unwrap().parse().unwrap(),
            );
            assert_eq!(structure, field(vector, "structHash"));
            let domain = domain_separator(
                &program,
                &store,
                &field(&fixture, "genesis"),
                &field(vector, "chainId"),
            );
            assert_eq!(intent_digest(&domain, &structure), field(vector, "digest"));
        }
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
            let nonce = vector["nonce"].as_str().unwrap().parse().unwrap();
            let deadline = vector["deadline"].as_str().unwrap().parse().unwrap();
            let domain = domain_separator(&program, &store, &genesis, &chain_id);
            let structure = cancel_struct_hash(&proxy, nonce, deadline);
            if vector["name"] == "first" {
                assert_ne!(structure, field(vector, "oldPairStructHash"));
                assert_ne!(
                    intent_digest(&domain, &structure),
                    field(vector, "oldPairDigest")
                );
            }
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

    #[test]
    fn independent_viem_vectors_match_withdraw_encoding() {
        let fixture: Value =
            serde_json::from_str(include_str!("../fixtures/withdraw-vectors.json")).unwrap();
        assert_eq!(fixture["generator"], "viem@2.57.7");
        assert_eq!(fixture["type"], WITHDRAW_TYPE);
        assert_eq!(fixture["vectors"].as_array().unwrap().len(), 12);
        for vector in fixture["vectors"].as_array().unwrap() {
            let program = Pubkey::new_from_array(field(vector, "program"));
            let store = Pubkey::new_from_array(field(vector, "store"));
            let genesis = field(vector, "genesis");
            let chain_id = field(vector, "chainId");
            let fields = WithdrawFields {
                proxy: Pubkey::new_from_array(field(vector, "proxy")),
                mint: Pubkey::new_from_array(field(vector, "mint")),
                amount: vector["amount"].as_str().unwrap().parse().unwrap(),
                decimals: vector["decimals"].as_u64().unwrap().try_into().unwrap(),
                recipient: Pubkey::new_from_array(field(vector, "recipient")),
                nonce: vector["nonce"].as_str().unwrap().parse().unwrap(),
                deadline: vector["deadline"].as_str().unwrap().parse().unwrap(),
            };
            assert_eq!(fields.mint.to_string(), vector["mintBase58"]);
            assert_eq!(fields.recipient.to_string(), vector["recipientBase58"]);
            let domain = domain_separator(&program, &store, &genesis, &chain_id);
            let structure = withdraw_struct_hash(&fields);
            assert_ne!(
                intent_digest(&domain, &structure),
                field(vector, "oldPairDigest")
            );
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
