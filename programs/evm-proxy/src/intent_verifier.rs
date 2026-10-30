use anchor_lang::{
    prelude::*,
    solana_program::{keccak, secp256k1_recover::secp256k1_recover},
};

use crate::{Config, IntentPair, ProxyError, ID};

const DOMAIN_TYPE: &[u8] = b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)";
const DOMAIN_NAME: &[u8] = b"GMX Solana EVM Proxy";
const DOMAIN_VERSION: &[u8] = b"1";
const CANCEL_TYPE: &[u8] =
    b"Cancel(bytes32 proxyAccount,uint64 generation,uint64 nonce,uint64 deadline)";
const EXECUTION_WINDOW: u64 = 3600;
const HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

pub(crate) fn cancel_struct_hash(proxy: &Pubkey, pair: IntentPair, deadline: u64) -> [u8; 32] {
    let generation = u64_word(pair.generation);
    let nonce = u64_word(pair.nonce);
    let deadline = u64_word(deadline);
    keccak::hashv(&[
        keccak::hash(CANCEL_TYPE).as_ref(),
        proxy.as_ref(),
        &generation,
        &nonce,
        &deadline,
    ])
    .to_bytes()
}

/// The caller supplies a struct hash computed from its own signed fields.
pub(crate) fn verify(
    config: &Config,
    expected_address: [u8; 20],
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
    let s: [u8; 32] = signature[32..]
        .try_into()
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    require!(
        s != [0; 32] && s <= HALF_ORDER,
        ProxyError::InvalidSignature
    );

    let salt = keccak::hashv(&[ID.as_ref(), config.store.as_ref(), &config.genesis_hash]);
    let domain = keccak::hashv(&[
        keccak::hash(DOMAIN_TYPE).as_ref(),
        keccak::hash(DOMAIN_NAME).as_ref(),
        keccak::hash(DOMAIN_VERSION).as_ref(),
        &chain_id,
        salt.as_ref(),
    ]);
    let digest = keccak::hashv(&[b"\x19\x01", domain.as_ref(), &struct_hash]);
    let public_key = secp256k1_recover(digest.as_ref(), recovery_id, &signature)
        .map_err(|_| error!(ProxyError::InvalidSignature))?;
    let address_hash = keccak::hash(public_key.to_bytes().as_ref());
    require!(
        address_hash.as_ref()[12..] == expected_address,
        ProxyError::InvalidSignature
    );
    Ok(())
}
