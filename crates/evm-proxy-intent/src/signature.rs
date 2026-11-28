use anchor_lang::solana_program::{keccak, secp256k1_recover::secp256k1_recover};

/// Raw 20-byte EVM address.
pub type EvmAddress = [u8; 20];

/// Largest canonical `s` scalar: (secp256k1 group order - 1) / 2, big-endian.
const SECP256K1_HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

/// Recover the EVM address that signed `digest` with a compact `r || s` signature.
///
/// Returns `None` when:
/// - `recovery_id` is not 0 or 1. EVM `ecrecover` accepts only `v` 27 or 28,
///   which are recovery IDs 0 and 1. The syscall also accepts 2 and 3, which
///   only fail in practice.
/// - `s` is zero or greater than half the group order, so each signature has
///   one accepted form.
/// - Recovery fails.
pub fn recover_signer(
    digest: &[u8; 32],
    signature: &[u8; 64],
    recovery_id: u8,
) -> Option<EvmAddress> {
    if recovery_id > 1 {
        return None;
    }
    let s = &signature[32..];
    if s == [0; 32] || s > &SECP256K1_HALF_ORDER[..] {
        return None;
    }
    let public_key = secp256k1_recover(digest, recovery_id, signature).ok()?;
    Some(evm_address(&public_key.to_bytes()))
}

/// EVM address of an uncompressed secp256k1 public key without its `0x04` prefix.
pub fn evm_address(public_key: &[u8; 64]) -> EvmAddress {
    let hash = keccak::hash(public_key).to_bytes();
    let mut address = EvmAddress::default();
    let start = hash.len() - address.len();
    address.copy_from_slice(&hash[start..]);
    address
}

#[cfg(test)]
mod tests {
    use anchor_lang::prelude::Pubkey;
    use libsecp256k1::{curve::Scalar, Message, PublicKey, SecretKey, Signature};

    use super::*;
    use crate::{digest, Cancel, Domain};

    const DOMAIN: Domain = Domain {
        program: Pubkey::new_from_array([1; 32]),
        store: Pubkey::new_from_array([2; 32]),
        genesis_hash: [3; 32],
        chain_id: [4; 32],
    };
    const INTENT: Cancel = Cancel {
        proxy_account: Pubkey::new_from_array([5; 32]),
        nonce: 6,
        deadline: 7,
    };

    fn key(seed: &[u8]) -> SecretKey {
        SecretKey::parse(&keccak::hash(seed).to_bytes()).unwrap()
    }

    fn address(key: &SecretKey) -> EvmAddress {
        let public_key = PublicKey::from_secret_key(key).serialize();
        evm_address(public_key[1..].try_into().unwrap())
    }

    fn sign(digest: &[u8; 32], key: &SecretKey) -> (Signature, u8) {
        let (signature, recovery_id) = libsecp256k1::sign(&Message::parse(digest), key);
        (signature, recovery_id.serialize())
    }

    #[test]
    fn half_order_is_half_the_group_order_rounded_down() {
        let mut half = Scalar::default();
        assert!(!bool::from(half.set_b32(&SECP256K1_HALF_ORDER)));
        // Scalars are reduced modulo the group order n, and n = 2 * half + 1.
        assert!((half + half + Scalar::from_int(1)).is_zero());
    }

    #[test]
    fn only_the_signer_key_recovers_the_signer() {
        let owner = key(b"owner");
        let digest = digest(&DOMAIN, &INTENT);
        let (signature, recovery_id) = sign(&digest, &owner);
        assert_eq!(
            recover_signer(&digest, &signature.serialize(), recovery_id),
            Some(address(&owner))
        );
        let (signature, recovery_id) = sign(&digest, &key(b"other"));
        assert_ne!(
            recover_signer(&digest, &signature.serialize(), recovery_id),
            Some(address(&owner))
        );
    }

    #[test]
    fn only_low_s_with_recovery_id_zero_or_one_is_accepted() {
        let owner = key(b"owner");
        let digest = digest(&DOMAIN, &INTENT);
        let (signature, recovery_id) = sign(&digest, &owner);
        let high_s = Signature {
            r: signature.r,
            s: -signature.s,
        }
        .serialize();
        let high_s_recovery_id = recovery_id ^ 1;
        let recovered = secp256k1_recover(&digest, high_s_recovery_id, &high_s).unwrap();
        assert_eq!(evm_address(&recovered.to_bytes()), address(&owner));
        assert_eq!(recover_signer(&digest, &high_s, high_s_recovery_id), None);
        for recovery_id in [2, 27] {
            assert_eq!(
                recover_signer(&digest, &signature.serialize(), recovery_id),
                None
            );
        }
    }

    #[test]
    fn a_signature_for_another_domain_does_not_recover_the_signer() {
        let owner = key(b"owner");
        let other = [9; 32];
        for domain in [
            Domain {
                program: Pubkey::new_from_array(other),
                ..DOMAIN
            },
            Domain {
                store: Pubkey::new_from_array(other),
                ..DOMAIN
            },
            Domain {
                genesis_hash: other,
                ..DOMAIN
            },
            Domain {
                chain_id: other,
                ..DOMAIN
            },
        ] {
            let (signature, recovery_id) = sign(&digest(&domain, &INTENT), &owner);
            assert_ne!(
                recover_signer(
                    &digest(&DOMAIN, &INTENT),
                    &signature.serialize(),
                    recovery_id
                ),
                Some(address(&owner))
            );
        }
    }
}
