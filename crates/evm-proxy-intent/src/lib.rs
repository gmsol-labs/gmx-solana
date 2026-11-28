//! Canonical EIP-712 encoding and signature rules for EVM proxy intents.

mod cancel;
mod signature;

pub use cancel::Cancel;
pub use signature::{evm_address, recover_signer, EvmAddress};

use anchor_lang::{prelude::Pubkey, solana_program::keccak};

/// Domain name shared by every proxy intent.
pub const DOMAIN_NAME: &str = "GMX Solana EVM Proxy";
/// Domain version shared by every proxy intent.
pub const DOMAIN_VERSION: &str = "1";
/// EIP-712 type of the domain.
pub const DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)";
/// Prefix of an EIP-712 signing digest.
pub const EIP712_PREFIX: &[u8; 2] = b"\x19\x01";
/// Maximum execution deadline lead over the current Clock, in seconds.
pub const EXECUTION_WINDOW_SECONDS: u64 = 3600;

/// Solana cluster genesis hash.
pub type GenesisHash = [u8; 32];
/// Full-width big-endian EVM chain ID.
pub type ChainId = [u8; 32];

/// EIP-712 domain of one deployment, Store, Solana cluster and EVM chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    /// EVM proxy program ID.
    pub program: Pubkey,
    /// Store bound to the deployment.
    pub store: Pubkey,
    /// Genesis hash of the Solana cluster.
    pub genesis_hash: GenesisHash,
    /// EVM chain the intent is signed for.
    pub chain_id: ChainId,
}

impl Domain {
    /// Salt binding the program, Store and Solana cluster.
    pub fn salt(&self) -> [u8; 32] {
        keccak::hashv(&[
            self.program.as_ref(),
            self.store.as_ref(),
            &self.genesis_hash,
        ])
        .to_bytes()
    }

    /// EIP-712 domain separator.
    pub fn separator(&self) -> [u8; 32] {
        keccak::hashv(&[
            keccak::hash(DOMAIN_TYPE.as_bytes()).as_ref(),
            keccak::hash(DOMAIN_NAME.as_bytes()).as_ref(),
            keccak::hash(DOMAIN_VERSION.as_bytes()).as_ref(),
            &self.chain_id,
            &self.salt(),
        ])
        .to_bytes()
    }
}

/// A signed EVM proxy intent.
pub trait Intent {
    /// EIP-712 type, the single source of the intent's field list.
    const TYPE: &'static str;

    /// EIP-712 struct hash.
    fn struct_hash(&self) -> [u8; 32];

    /// Proxy nonce consumed by the intent.
    fn nonce(&self) -> u64;

    /// Latest Unix timestamp at which the intent executes.
    fn deadline(&self) -> u64;
}

/// Digest signed by the EVM owner.
pub fn digest(domain: &Domain, intent: &impl Intent) -> [u8; 32] {
    keccak::hashv(&[EIP712_PREFIX, &domain.separator(), &intent.struct_hash()]).to_bytes()
}

/// Big-endian EIP-712 word of an unsigned integer.
pub fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

/// Name and `(name, type)` members of an EIP-712 type such as [`DOMAIN_TYPE`].
pub fn type_members(encoded: &str) -> Option<(&str, impl Iterator<Item = (&str, &str)>)> {
    let (name, members) = encoded.strip_suffix(')')?.split_once('(')?;
    let members = members.split(',').filter_map(|member| {
        let (ty, name) = member.split_once(' ')?;
        Some((name, ty))
    });
    Some((name, members))
}

/// Whether `deadline` lies in the inclusive window from `now` through
/// `now + EXECUTION_WINDOW_SECONDS`. A negative `now` has no window.
pub fn is_within_execution_window(now: i64, deadline: u64) -> bool {
    u64::try_from(now).is_ok_and(|now| {
        deadline
            .checked_sub(now)
            .is_some_and(|lead| lead <= EXECUTION_WINDOW_SECONDS)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_members_split_the_type_string() {
        let (name, members) = type_members(DOMAIN_TYPE).unwrap();
        assert_eq!(name, "EIP712Domain");
        assert_eq!(
            members.collect::<Vec<_>>(),
            [
                ("name", "string"),
                ("version", "string"),
                ("chainId", "uint256"),
                ("salt", "bytes32"),
            ]
        );
    }

    #[test]
    fn execution_window_includes_both_ends() {
        const NOW: u64 = 7200;
        let now = i64::try_from(NOW).unwrap();
        assert!(!is_within_execution_window(now, NOW - 1));
        assert!(is_within_execution_window(now, NOW));
        assert!(is_within_execution_window(
            now,
            NOW + EXECUTION_WINDOW_SECONDS
        ));
        assert!(!is_within_execution_window(
            now,
            NOW + EXECUTION_WINDOW_SECONDS + 1
        ));
    }

    #[test]
    fn execution_window_rejects_a_negative_clock_and_does_not_overflow() {
        assert!(!is_within_execution_window(-1, 0));
        let latest_now = u64::try_from(i64::MAX).unwrap();
        assert!(is_within_execution_window(
            i64::MAX,
            latest_now + EXECUTION_WINDOW_SECONDS
        ));
        assert!(!is_within_execution_window(i64::MAX, u64::MAX));
    }
}
