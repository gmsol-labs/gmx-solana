use anchor_lang::{
    prelude::*,
    solana_program::{keccak, secp256k1_recover::secp256k1_recover},
};
use gmsol_evm_proxy_intent::{domain_separator, intent_digest};

use crate::{Config, ProxyError, EVM_ADDRESS_LEN, ID};

const SECP256K1_SCALAR_LEN: usize = 32;
const EVM_ADDRESS_HASH_START: usize = 32 - EVM_ADDRESS_LEN;

const EXECUTION_WINDOW: u64 = 3600;
/// Largest canonical s scalar: (secp256k1 group order - 1) / 2, big-endian.
pub(crate) const SECP256K1_HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

fn is_low_s(s: &[u8; 32]) -> bool {
    *s != [0; 32] && *s <= SECP256K1_HALF_ORDER
}

/// The caller supplies a struct hash computed from its own signed fields.
pub(crate) fn verify(
    config: &Config,
    expected_address: [u8; EVM_ADDRESS_LEN],
    chain_id: [u8; 32],
    deadline: u64,
    struct_hash: [u8; 32],
    signature: [u8; 64],
    recovery_id: u8,
) -> Result<()> {
    require!(!config.paused, ProxyError::Paused);
    require!(
        config.allowed_chain_ids.contains(&chain_id),
        ProxyError::ChainNotAllowed
    );
    let now = u64::try_from(Clock::get()?.unix_timestamp)
        .map_err(|_| error!(ProxyError::InvalidDeadline))?;
    let latest = now
        .checked_add(EXECUTION_WINDOW)
        .ok_or(error!(ProxyError::InvalidDeadline))?;
    require!(
        (now..=latest).contains(&deadline),
        ProxyError::InvalidDeadline
    );
    require!(recovery_id <= 1, ProxyError::InvalidSignature);
    let s: [u8; 32] = signature[SECP256K1_SCALAR_LEN..]
        .try_into()
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    require!(is_low_s(&s), ProxyError::InvalidSignature);

    let domain = domain_separator(&ID, &config.store, &config.genesis_hash, &chain_id);
    let digest = intent_digest(&domain, &struct_hash);
    let public_key = secp256k1_recover(&digest, recovery_id, &signature)
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    let address_hash = keccak::hash(public_key.to_bytes().as_ref());
    require!(
        address_hash.as_ref()[EVM_ADDRESS_HASH_START..] == expected_address,
        ProxyError::InvalidSignature
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent secp256k1 order from SEC 2, not derived from the policy bound.
    const SECP256K1_ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];

    #[test]
    fn low_s_bound_is_half_of_the_independent_group_order() {
        let mut doubled_plus_one = [0; 32];
        let mut carry = 1u16;
        for index in (0..32).rev() {
            let value = u16::from(SECP256K1_HALF_ORDER[index]) * 2 + carry;
            doubled_plus_one[index] = value as u8;
            carry = value >> 8;
        }
        assert_eq!(carry, 0);
        assert_eq!(doubled_plus_one, SECP256K1_ORDER);
    }

    #[test]
    fn low_s_accepts_the_exact_bound_and_rejects_zero_and_larger_scalars() {
        let mut one = [0; 32];
        one[31] = 1;
        assert!(is_low_s(&one));
        assert!(is_low_s(&SECP256K1_HALF_ORDER));
        let mut above_bound = SECP256K1_HALF_ORDER;
        above_bound[31] += 1;
        assert!(!is_low_s(&[0; 32]));
        assert!(!is_low_s(&above_bound));
        assert!(!is_low_s(&SECP256K1_ORDER));
        assert!(!is_low_s(&[u8::MAX; 32]));
    }
}
